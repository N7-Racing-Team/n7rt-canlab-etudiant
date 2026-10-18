//! Parameter and state values (contract §4): coercion of an edit value (typed or text) and validation of a stored
//! value (check, load). No value is ever corrected silently: anything outside the rules is refused.

use crate::units::{self, Parsed, TextError};
use crate::{ParamDef, RefusalCode, StateDef, Value, ValueType, SAMPLE_RATES_HZ};

/// A refused value: code and French message.
pub(crate) type Fail = (RefusalCode, String);

/// Int parameters whose value must divide 1000 Hz (sampled at the end of a 1 ms frame).
fn is_sample_rate(def: &ParamDef) -> bool {
    matches!(def.name, "sample_rate" | "adc_sample_rate")
}

fn sample_rate_message() -> String {
    let list: Vec<String> = SAMPLE_RATES_HZ.iter().map(|r| r.to_string()).collect();
    format!("La fr\u{e9}quence d'\u{e9}chantillonnage doit diviser 1000\u{a0}Hz ({})", list.join(", "))
}

fn range_text(def: &ParamDef) -> (String, String) {
    match def.vtype {
        ValueType::Int => (units::integer(def.min as i64, def.unit), units::integer(def.max as i64, def.unit)),
        _ => (units::quantity(def.min, def.unit), units::quantity(def.max, def.unit)),
    }
}

fn number_out_of_range(def: &ParamDef, shown: &str) -> Fail {
    let (min, max) = range_text(def);
    let mut msg = units::out_of_range(shown, &min, &max);
    if def.vtype == ValueType::NumberOrInf {
        msg.push_str(" (ou \u{221e})");
    }
    (RefusalCode::ParamOutOfRange, msg)
}

fn text_fail(def: &ParamDef, text: &str, e: TextError) -> Fail {
    match e {
        TextError::Unit if def.unit.is_empty() => (RefusalCode::ParamUnit, "aucune unit\u{e9} attendue".to_string()),
        TextError::Unit => (RefusalCode::ParamUnit, format!("unit\u{e9} attendue : {}", def.unit)),
        TextError::Syntax if text.trim().is_empty() => {
            (RefusalCode::ParamParse, format!("{} : saisie vide, une valeur est attendue", def.label_fr))
        }
        TextError::Syntax => (
            RefusalCode::ParamParse,
            format!("Saisie illisible : \u{ab}\u{a0}{}\u{a0}\u{bb} (exemples : 4.7k, 4k7, 2,2\u{a0}\u{b5})", text.trim()),
        ),
    }
}

/// A finite or infinite number for a Number / NumberOrInf parameter, range-checked; −0 becomes 0.
fn checked_number(def: &ParamDef, x: f64, shown: Option<&str>) -> Result<Value, Fail> {
    if x.is_nan() {
        return Err((RefusalCode::ParamOutOfRange, format!("{} : valeur non finie", def.label_fr)));
    }
    if x == f64::INFINITY && def.vtype == ValueType::NumberOrInf {
        return Ok(Value::Num(x));
    }
    let shown = shown.map_or_else(|| units::quantity(x, def.unit), str::to_string);
    if !x.is_finite() || x < def.min || x > def.max {
        return Err(number_out_of_range(def, &shown));
    }
    Ok(Value::Num(if x == 0.0 { 0.0 } else { x }))
}

fn checked_int(def: &ParamDef, i: i64) -> Result<Value, Fail> {
    if is_sample_rate(def) {
        return if SAMPLE_RATES_HZ.contains(&i) { Ok(Value::Int(i)) } else { Err((RefusalCode::ParamOutOfRange, sample_rate_message())) };
    }
    if (i as f64) < def.min || (i as f64) > def.max {
        return Err(number_out_of_range(def, &units::integer(i, def.unit)));
    }
    Ok(Value::Int(i))
}

fn integral(x: f64) -> Option<i64> {
    (x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15).then_some(x as i64)
}

fn type_fail(def: &ParamDef, expected: &str) -> Fail {
    (RefusalCode::ParamType, format!("{} : {expected} attendu", def.label_fr))
}

/// Value of an edit (add_component params, set_param) for parameter `def`: typed, or a text parsed with the unit
/// grammar for numeric parameters. Returns the value as stored.
pub(crate) fn coerce(def: &ParamDef, value: &Value) -> Result<Value, Fail> {
    match def.vtype {
        ValueType::Number | ValueType::NumberOrInf => match value {
            Value::Num(x) => checked_number(def, *x, None),
            Value::Int(i) => checked_number(def, *i as f64, None),
            Value::Choice(text) => match units::parse(text, def.unit) {
                Ok(Parsed::Num(x)) if x.is_finite() => checked_number(def, x, None),
                Ok(Parsed::Num(_)) => Err(number_out_of_range(def, &format!("\u{ab}\u{a0}{}\u{a0}\u{bb}", text.trim()))),
                Ok(Parsed::Inf { negative: false }) => checked_number(def, f64::INFINITY, None),
                Ok(Parsed::Inf { negative: true }) => checked_number(def, f64::NEG_INFINITY, None),
                Err(e) => Err(text_fail(def, text, e)),
            },
            Value::Bool(_) => Err(type_fail(def, "nombre")),
        },
        ValueType::Int => match value {
            Value::Int(i) => checked_int(def, *i),
            Value::Num(x) => integral(*x).map_or_else(|| Err(type_fail(def, "entier")), |i| checked_int(def, i)),
            Value::Choice(text) => match units::parse(text, def.unit) {
                Ok(Parsed::Num(x)) => integral(x).map_or_else(|| Err(type_fail(def, "entier")), |i| checked_int(def, i)),
                Ok(Parsed::Inf { .. }) => Err(type_fail(def, "entier")),
                Err(e) => Err(text_fail(def, text, e)),
            },
            Value::Bool(_) => Err(type_fail(def, "entier")),
        },
        ValueType::Bool => match value {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            _ => Err(type_fail(def, "vrai ou faux")),
        },
        ValueType::Choice => {
            let text = match value {
                Value::Choice(s) => s.clone(),
                Value::Int(i) => i.to_string(),
                Value::Num(x) => match integral(*x) {
                    Some(i) => i.to_string(),
                    None => return Err(choice_fail(def)),
                },
                Value::Bool(_) => return Err(type_fail(def, "choix")),
            };
            if def.choices.contains(&text.as_str()) {
                Ok(Value::Choice(text))
            } else {
                Err(choice_fail(def))
            }
        }
    }
}

fn choice_fail(def: &ParamDef) -> Fail {
    (RefusalCode::ParamOutOfRange, format!("{} : valeurs permises {}", def.label_fr, def.choices.join(", ")))
}

/// Stored value of a parameter (exact type, range): code and message of the violation.
pub(crate) fn check_stored(def: &ParamDef, value: &Value) -> Result<(), Fail> {
    let typed = match (def.vtype, value) {
        (ValueType::Number | ValueType::NumberOrInf, Value::Num(_))
        | (ValueType::Int, Value::Int(_))
        | (ValueType::Bool, Value::Bool(_))
        | (ValueType::Choice, Value::Choice(_)) => true,
        _ => false,
    };
    if !typed {
        let expected = match def.vtype {
            ValueType::Number | ValueType::NumberOrInf => "nombre",
            ValueType::Int => "entier",
            ValueType::Bool => "vrai ou faux",
            ValueType::Choice => "choix",
        };
        return Err(type_fail(def, expected));
    }
    coerce(def, value).map(|_| ())
}

/// Stored value of an interaction state.
pub(crate) fn check_state(def: &StateDef, value: &Value) -> Result<(), Fail> {
    match (def.vtype, value) {
        (ValueType::Bool, Value::Bool(_)) => Ok(()),
        (ValueType::Number, Value::Num(x)) => {
            if !(x.is_finite() && *x >= def.min && *x <= def.max) {
                return Err((
                    RefusalCode::StateOutOfRange,
                    units::out_of_range(&units::quantity(*x, def.unit), &units::quantity(def.min, def.unit), &units::quantity(def.max, def.unit)),
                ));
            }
            // The wiper position is k/1000 with k integral (contract §2.1).
            if def.name == "position" && ((x * 1000.0).round() / 1000.0) != *x {
                return Err((RefusalCode::StateOutOfRange, format!("{} : pas de 0.001 attendu ({x})", def.label_fr)));
            }
            Ok(())
        }
        _ => Err((RefusalCode::StateOutOfRange, format!("{} : valeur de type inattendu", def.label_fr))),
    }
}

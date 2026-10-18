//! Text values of parameters (contract §4), through `set_param` as the UI sends them: exact conversion, grammar,
//! units, special parameters and every error message.

mod common;

use common::{add, doc, edit, refused};
use n7rt_circuit::{CompId, Document, Edit, Kind, RefusalCode, Rot, Value};

const PICO: [f64; 36] = [1.0e-12, 1.2e-12, 1.5e-12, 1.8e-12, 2.2e-12, 2.7e-12, 3.3e-12, 3.9e-12, 4.7e-12, 5.6e-12, 6.8e-12, 8.2e-12, 10e-12, 12e-12, 15e-12, 18e-12, 22e-12, 27e-12, 33e-12, 39e-12, 47e-12, 56e-12, 68e-12, 82e-12, 100e-12, 120e-12, 150e-12, 180e-12, 220e-12, 270e-12, 330e-12, 390e-12, 470e-12, 560e-12, 680e-12, 820e-12];
const NANO: [f64; 36] = [1.0e-9, 1.2e-9, 1.5e-9, 1.8e-9, 2.2e-9, 2.7e-9, 3.3e-9, 3.9e-9, 4.7e-9, 5.6e-9, 6.8e-9, 8.2e-9, 10e-9, 12e-9, 15e-9, 18e-9, 22e-9, 27e-9, 33e-9, 39e-9, 47e-9, 56e-9, 68e-9, 82e-9, 100e-9, 120e-9, 150e-9, 180e-9, 220e-9, 270e-9, 330e-9, 390e-9, 470e-9, 560e-9, 680e-9, 820e-9];
const MICRO: [f64; 36] = [1.0e-6, 1.2e-6, 1.5e-6, 1.8e-6, 2.2e-6, 2.7e-6, 3.3e-6, 3.9e-6, 4.7e-6, 5.6e-6, 6.8e-6, 8.2e-6, 10e-6, 12e-6, 15e-6, 18e-6, 22e-6, 27e-6, 33e-6, 39e-6, 47e-6, 56e-6, 68e-6, 82e-6, 100e-6, 120e-6, 150e-6, 180e-6, 220e-6, 270e-6, 330e-6, 390e-6, 470e-6, 560e-6, 680e-6, 820e-6];
const MILLI: [f64; 36] = [1.0e-3, 1.2e-3, 1.5e-3, 1.8e-3, 2.2e-3, 2.7e-3, 3.3e-3, 3.9e-3, 4.7e-3, 5.6e-3, 6.8e-3, 8.2e-3, 10e-3, 12e-3, 15e-3, 18e-3, 22e-3, 27e-3, 33e-3, 39e-3, 47e-3, 56e-3, 68e-3, 82e-3, 100e-3, 120e-3, 150e-3, 180e-3, 220e-3, 270e-3, 330e-3, 390e-3, 470e-3, 560e-3, 680e-3, 820e-3];
const UNIT: [f64; 36] = [1.0e0, 1.2e0, 1.5e0, 1.8e0, 2.2e0, 2.7e0, 3.3e0, 3.9e0, 4.7e0, 5.6e0, 6.8e0, 8.2e0, 10e0, 12e0, 15e0, 18e0, 22e0, 27e0, 33e0, 39e0, 47e0, 56e0, 68e0, 82e0, 100e0, 120e0, 150e0, 180e0, 220e0, 270e0, 330e0, 390e0, 470e0, 560e0, 680e0, 820e0];
const KILO: [f64; 36] = [1.0e3, 1.2e3, 1.5e3, 1.8e3, 2.2e3, 2.7e3, 3.3e3, 3.9e3, 4.7e3, 5.6e3, 6.8e3, 8.2e3, 10e3, 12e3, 15e3, 18e3, 22e3, 27e3, 33e3, 39e3, 47e3, 56e3, 68e3, 82e3, 100e3, 120e3, 150e3, 180e3, 220e3, 270e3, 330e3, 390e3, 470e3, 560e3, 680e3, 820e3];
const MEGA: [f64; 36] = [1.0e6, 1.2e6, 1.5e6, 1.8e6, 2.2e6, 2.7e6, 3.3e6, 3.9e6, 4.7e6, 5.6e6, 6.8e6, 8.2e6, 10e6, 12e6, 15e6, 18e6, 22e6, 27e6, 33e6, 39e6, 47e6, 56e6, 68e6, 82e6, 100e6, 120e6, 150e6, 180e6, 220e6, 270e6, 330e6, 390e6, 470e6, 560e6, 680e6, 820e6];
const GIGA: [f64; 36] = [1.0e9, 1.2e9, 1.5e9, 1.8e9, 2.2e9, 2.7e9, 3.3e9, 3.9e9, 4.7e9, 5.6e9, 6.8e9, 8.2e9, 10e9, 12e9, 15e9, 18e9, 22e9, 27e9, 33e9, 39e9, 47e9, 56e9, 68e9, 82e9, 100e9, 120e9, 150e9, 180e9, 220e9, 270e9, 330e9, 390e9, 470e9, 560e9, 680e9, 820e9];

const MANTISSAS: [&str; 36] = [
    "1.0", "1.2", "1.5", "1.8", "2.2", "2.7", "3.3", "3.9", "4.7", "5.6", "6.8", "8.2", "10", "12", "15", "18", "22", "27", "33",
    "39", "47", "56", "68", "82", "100", "120", "150", "180", "220", "270", "330", "390", "470", "560", "680", "820",
];

const NB: char = '\u{a0}';
const OHM: &str = "\u{3a9}";

struct Bench {
    d: Document,
    c: CompId,
    r: CompId,
    bp: CompId,
    adc: CompId,
    ecu: CompId,
    rt: CompId,
    bus: CompId,
    v: CompId,
}

fn bench() -> Bench {
    let mut d = doc();
    let c = add(&mut d, Kind::Capacitor, 0, 0, Rot::R0);
    let r = add(&mut d, Kind::Resistor, 20, 0, Rot::R0);
    let bp = add(&mut d, Kind::PushButton, 40, 0, Rot::R0);
    let adc = add(&mut d, Kind::AdcInput, 60, 0, Rot::R0);
    let ecu = add(&mut d, Kind::EcuStm32, 0, 40, Rot::R0);
    let rt = add(&mut d, Kind::Termination, 40, 40, Rot::R0);
    let bus = add(&mut d, Kind::CanBus, 60, 40, Rot::R0);
    let v = add(&mut d, Kind::DcSupply, 80, 0, Rot::R0);
    Bench { d, c, r, bp, adc, ecu, rt, bus, v }
}

fn set(d: &mut Document, comp: CompId, name: &str, value: Value) -> Result<Value, (RefusalCode, String)> {
    let rev = d.rev();
    match d.edit(rev, &Edit::SetParam { comp, name: name.to_string(), value }) {
        Ok(_) => Ok(d.circuit().components()[&comp].params[name].clone()),
        Err(r) => Err((r.code, r.message_fr)),
    }
}

fn text(d: &mut Document, comp: CompId, name: &str, t: &str) -> Result<Value, (RefusalCode, String)> {
    set(d, comp, name, Value::Choice(t.to_string()))
}

fn num(d: &mut Document, comp: CompId, name: &str, t: &str) -> f64 {
    match text(d, comp, name, t) {
        Ok(Value::Num(x)) => x,
        other => panic!("{t:?}: {other:?}"),
    }
}

fn same_bits(got: f64, want: f64, what: &str) {
    assert_eq!(got.to_bits(), want.to_bits(), "{what}: {got:e} instead of {want:e}");
}

#[test]
fn mechanical_unit_suffix_is_read_before_metric_prefix() {
    let mut d = doc();
    let wheel = add(&mut d, Kind::Ats682Lsh, 0, 0, Rot::R0);
    assert_eq!(text(&mut d,wheel,"air_gap_mm","1 mm"),Ok(Value::Num(1.0)));
    let column = add(&mut d,Kind::SteeringColumn,20,0,Rot::R0);
    assert_eq!(text(&mut d,column,"max_rate_deg_per_s","120 °/s"),Ok(Value::Num(120.0)));
}

#[test]
fn every_e12_value_with_every_prefix_is_the_literal_bit_for_bit() {
    let mut b = bench();
    let (c, r, bp) = (b.c, b.r, b.bp);
    let d = &mut b.d;
    let cases: [(&str, &[f64; 36], CompId, &str, &str); 10] = [
        ("p", &PICO, c, "capacitance", "F"),
        ("n", &NANO, c, "capacitance", "F"),
        ("u", &MICRO, c, "capacitance", "F"),
        ("\u{b5}", &MICRO, c, "capacitance", "F"),
        ("\u{3bc}", &MICRO, c, "capacitance", "F"),
        ("m", &MILLI, c, "capacitance", "F"),
        ("", &UNIT, r, "resistance", OHM),
        ("k", &KILO, r, "resistance", OHM),
        ("M", &MEGA, bp, "r_off", OHM),
        ("G", &GIGA, bp, "r_off", OHM),
    ];
    for (prefix, table, comp, name, unit) in cases {
        for (m, want) in MANTISSAS.iter().zip(table.iter()) {
            for t in [format!("{m}{prefix}"), format!("{m} {prefix}{unit}"), format!("{}{prefix}{unit}", m.replace('.', ","))] {
                same_bits(num(d, comp, name, &t), *want, &t);
            }
        }
    }
    // The reason for the rule: multiplying by the prefix is not exact.
    assert_ne!((2.2f64 * 1e-9).to_bits(), 2.2e-9f64.to_bits());
    same_bits(num(d, c, "capacitance", "2.2n"), 2.2e-9, "2.2n");
}

#[test]
fn rkm_notation() {
    let mut b = bench();
    let (c, r, bp) = (b.c, b.r, b.bp);
    let d = &mut b.d;
    for (t, want) in [("4k7", 4.7e3), ("2R2", 2.2), ("1M5", 1.5e6), ("R47", 0.47), ("47R", 47.0), ("4k7 \u{3a9}", 4.7e3), ("2R2ohm", 2.2)] {
        same_bits(num(d, r, "resistance", t), want, t);
    }
    for (t, want) in [("4\u{b5}7", 4.7e-6), ("6p8", 6.8e-12), ("2n2", 2.2e-9), ("1m5", 1.5e-3), ("3u3F", 3.3e-6)] {
        same_bits(num(d, c, "capacitance", t), want, t);
    }
    same_bits(num(d, bp, "r_off", "1G5"), 1.5e9, "1G5");
}

#[test]
fn signs_separators_exponents_spaces_and_unit_spellings() {
    let mut b = bench();
    let (c, r, v) = (b.c, b.r, b.v);
    let d = &mut b.d;
    for (t, want) in [
        (" 4.7 k \u{3a9} ", 4.7e3),
        ("4.7k\u{2126}", 4.7e3),
        ("4.7 kohm", 4.7e3),
        ("4.7 kOhm", 4.7e3),
        ("47 ohms", 47.0),
        ("4.7e3", 4.7e3),
        ("47E-1", 4.7),
        ("1e+3 \u{3a9}", 1e3),
        ("+220", 220.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("1,5k", 1.5e3),
        ("1\u{a0}k\u{3a9}", 1e3),
    ] {
        same_bits(num(d, r, "resistance", t), want, t);
    }
    same_bits(num(d, c, "capacitance", "1e\u{2212}3"), 1e-3, "exponent with U+2212");
    same_bits(num(d, c, "capacitance", "10uf"), 10e-6, "10uf");
    same_bits(num(d, c, "capacitance", "1e3n"), 1e-6, "exponent and prefix");
    same_bits(num(d, v, "voltage", "3.3v"), 3.3, "3.3v");
    same_bits(num(d, v, "voltage", "500 mV"), 0.5, "500 mV");
    // −0 is stored as +0, never as a negative zero.
    let z = num(d, v, "voltage", "\u{2212}0");
    assert_eq!(z.to_bits(), 0.0f64.to_bits());
    // A minus sign U+2212 is read as negative.
    assert_eq!(
        text(d, v, "voltage", "\u{2212}5 V"),
        Err((RefusalCode::ParamOutOfRange, format!("\u{2212}5{NB}V hors plage [0{NB}V ; 30{NB}V]")))
    );
    assert_eq!(text(d, v, "voltage", "- 5"), Err((RefusalCode::ParamOutOfRange, format!("\u{2212}5{NB}V hors plage [0{NB}V ; 30{NB}V]"))));
}

#[test]
fn infinity_only_for_number_or_inf() {
    let mut b = bench();
    let (adc, r) = (b.adc, b.r);
    let d = &mut b.d;
    for t in ["inf", "\u{221e}", "INF \u{3a9}", "+inf", " \u{221e} ohm"] {
        assert_eq!(num(d, adc, "r_in", "10k"), 1e4);
        assert_eq!(num(d, adc, "r_in", t), f64::INFINITY, "{t}");
    }
    // Only "inf" and "∞" are in the grammar.
    assert_eq!(
        text(d, adc, "r_in", "Infinity"),
        Err((RefusalCode::ParamParse, format!("Saisie illisible : \u{ab}{NB}Infinity{NB}\u{bb} (exemples : 4.7k, 4k7, 2,2{NB}\u{b5})")))
    );
    assert_eq!(
        text(d, adc, "r_in", "100"),
        Err((RefusalCode::ParamOutOfRange, format!("100{NB}{OHM} hors plage [1{NB}k{OHM} ; 1{NB}T{OHM}] (ou \u{221e})")))
    );
    assert_eq!(
        text(d, adc, "r_in", "\u{2212}inf"),
        Err((RefusalCode::ParamOutOfRange, format!("\u{2212}\u{221e} hors plage [1{NB}k{OHM} ; 1{NB}T{OHM}] (ou \u{221e})")))
    );
    assert_eq!(text(d, r, "resistance", "inf"), Err((RefusalCode::ParamOutOfRange, format!("\u{221e} hors plage [10{NB}m{OHM} ; 100{NB}M{OHM}]"))));
    // An overflowing literal is not ∞.
    assert_eq!(
        text(d, adc, "r_in", "1e999"),
        Err((RefusalCode::ParamOutOfRange, format!("\u{ab}{NB}1e999{NB}\u{bb} hors plage [1{NB}k{OHM} ; 1{NB}T{OHM}] (ou \u{221e})")))
    );
}

#[test]
fn every_error_message_is_exact() {
    let mut b = bench();
    let (r, c, adc, ecu, rt, bus, v) = (b.r, b.c, b.adc, b.ecu, b.rt, b.bus, b.v);
    let d = &mut b.d;
    let parse = |t: &str| (RefusalCode::ParamParse, format!("Saisie illisible : \u{ab}{NB}{t}{NB}\u{bb} (exemples : 4.7k, 4k7, 2,2{NB}\u{b5})"));
    // §3.6: « 0 Ω hors plage [10 mΩ ; 100 MΩ] », U+00A0 before the unit.
    assert_eq!(text(d, r, "resistance", "0"), Err((RefusalCode::ParamOutOfRange, format!("0{NB}{OHM} hors plage [10{NB}m{OHM} ; 100{NB}M{OHM}]"))));
    assert_eq!(text(d, r, "resistance", "200M"), Err((RefusalCode::ParamOutOfRange, format!("200{NB}M{OHM} hors plage [10{NB}m{OHM} ; 100{NB}M{OHM}]"))));
    assert_eq!(text(d, r, "power_rating", "50m"), Err((RefusalCode::ParamOutOfRange, format!("50{NB}mW hors plage [62.5{NB}mW ; 10{NB}W]"))));
    // Below the p…T prefixes: scientific notation, never a rounded value.
    assert_eq!(text(d, c, "capacitance", "0.5p"), Err((RefusalCode::ParamOutOfRange, format!("5e\u{2212}13{NB}F hors plage [1{NB}pF ; 1{NB}F]"))));
    // Units (§4: « unité attendue : Ω »).
    assert_eq!(text(d, r, "resistance", "5 V"), Err((RefusalCode::ParamUnit, format!("unit\u{e9} attendue : {OHM}"))));
    assert_eq!(text(d, r, "resistance", "4.7K"), Err((RefusalCode::ParamUnit, format!("unit\u{e9} attendue : {OHM}"))));
    assert_eq!(text(d, r, "resistance", "4k7k"), Err((RefusalCode::ParamUnit, format!("unit\u{e9} attendue : {OHM}"))));
    assert_eq!(text(d, c, "capacitance", "10 \u{b5}H"), Err((RefusalCode::ParamUnit, "unit\u{e9} attendue : F".to_string())));
    assert_eq!(text(d, bus, "remote_nodes", "3 V"), Err((RefusalCode::ParamUnit, "aucune unit\u{e9} attendue".to_string())));
    assert_eq!(text(d, r, "resistance", "inf V"), Err((RefusalCode::ParamUnit, format!("unit\u{e9} attendue : {OHM}"))));
    // Syntax.
    for t in ["abc", "4..7", "1e", "4k7.5", "k7", "--5", "4,7,1", "1 e3", "0x10", "R", "5 V V"] {
        assert_eq!(text(d, r, "resistance", t), Err(parse(t)), "{t}");
    }
    assert_eq!(text(d, r, "resistance", ""), Err((RefusalCode::ParamParse, "R\u{e9}sistance : saisie vide, une valeur est attendue".to_string())));
    assert_eq!(text(d, r, "resistance", "  "), Err((RefusalCode::ParamParse, "R\u{e9}sistance : saisie vide, une valeur est attendue".to_string())));
    // Types.
    assert_eq!(set(d, r, "resistance", Value::Bool(true)), Err((RefusalCode::ParamType, "R\u{e9}sistance : nombre attendu".to_string())));
    assert_eq!(set(d, r, "resistance", Value::Num(f64::NAN)), Err((RefusalCode::ParamOutOfRange, "R\u{e9}sistance : valeur non finie".to_string())));
    assert_eq!(set(d, c, "polarized", Value::Choice("oui".into())), Err((RefusalCode::ParamType, "Polaris\u{e9} (broche 1 = +) : vrai ou faux attendu".to_string())));
    assert_eq!(set(d, adc, "bits", Value::Num(12.5)), Err((RefusalCode::ParamType, "R\u{e9}solution : entier attendu".to_string())));
    // Special rules (§4).
    let rates = "La fr\u{e9}quence d'\u{e9}chantillonnage doit diviser 1000\u{a0}Hz (1, 2, 4, 5, 8, 10, 20, 25, 40, 50, 100, 125, 200, 250, 500, 1000)";
    assert_eq!(set(d, adc, "sample_rate", Value::Int(3)), Err((RefusalCode::ParamOutOfRange, rates.to_string())));
    assert_eq!(text(d, ecu, "adc_sample_rate", "300 Hz"), Err((RefusalCode::ParamOutOfRange, rates.to_string())));
    assert_eq!(text(d, adc, "sample_rate", "2.5"), Err((RefusalCode::ParamType, "Fr\u{e9}quence d'\u{e9}chantillonnage : entier attendu".to_string())));
    assert_eq!(set(d, adc, "bits", Value::Int(17)), Err((RefusalCode::ParamOutOfRange, format!("17{NB}bit hors plage [8{NB}bit ; 16{NB}bit]"))));
    assert_eq!(
        set(d, ecu, "adc_bits", Value::Choice("11".into())),
        Err((RefusalCode::ParamOutOfRange, "R\u{e9}solution ADC : valeurs permises 8, 10, 12, 14, 16".to_string()))
    );
    assert_eq!(set(d, bus, "remote_nodes", Value::Int(31)), Err((RefusalCode::ParamOutOfRange, "31 hors plage [0 ; 30]".to_string())));
    assert_eq!(
        set(d, ecu, "can_mode", Value::Choice("lin".into())),
        Err((RefusalCode::ParamOutOfRange, "Broches FDCAN : valeurs permises can, hi_z".to_string()))
    );
    assert_eq!(text(d, rt, "resistance", "120"), Err((RefusalCode::ParamLocked, format!("R\u{e9}sistance fix\u{e9}e par le composant (120{NB}{OHM})"))));
    assert_eq!(text(d, v, "foo", "1"), Err((RefusalCode::ParamUnknown, "Param\u{e8}tre \u{ab}\u{a0}foo\u{a0}\u{bb} inconnu pour Alimentation DC".to_string())));
}

#[test]
fn typed_values_and_special_parameters() {
    let mut b = bench();
    let (r, adc, ecu, c) = (b.r, b.adc, b.ecu, b.c);
    let d = &mut b.d;
    assert_eq!(set(d, r, "resistance", Value::Int(4700)), Ok(Value::Num(4700.0)));
    assert_eq!(set(d, adc, "sample_rate", Value::Choice("1 kHz".into())), Ok(Value::Int(1000)));
    assert_eq!(set(d, adc, "sample_rate", Value::Choice("250 Hz".into())), Ok(Value::Int(250)));
    assert_eq!(set(d, adc, "sample_rate", Value::Num(125.0)), Ok(Value::Int(125)));
    assert_eq!(set(d, adc, "bits", Value::Choice("12 bit".into())), Ok(Value::Int(12)));
    assert_eq!(set(d, ecu, "adc_bits", Value::Int(16)), Ok(Value::Choice("16".into())));
    assert_eq!(set(d, ecu, "adc_bits", Value::Choice("8".into())), Ok(Value::Choice("8".into())));
    assert_eq!(set(d, ecu, "adc_sample_rate", Value::Int(40)), Ok(Value::Int(40)));
    assert_eq!(set(d, c, "polarized", Value::Bool(true)), Ok(Value::Bool(true)));
    // Every editable parameter keeps exactly the parsed value: nothing is rounded or clamped.
    assert_eq!(set(d, r, "resistance", Value::Choice("100.000001".into())), Ok(Value::Num(100.000001)));
}

#[test]
fn a_refused_value_changes_nothing() {
    let mut b = bench();
    let r = b.r;
    let before = b.d.circuit().clone();
    let rev = b.d.rev();
    let ref_ = refused(&mut b.d, Edit::SetParam { comp: r, name: "resistance".into(), value: Value::Choice("0".into()) });
    assert_eq!(ref_.code, RefusalCode::ParamOutOfRange);
    assert_eq!(b.d.circuit(), &before);
    assert_eq!(b.d.rev(), rev);
    // The same value again is an empty patch: no revision, no history entry.
    let a = edit(&mut b.d, Edit::SetParam { comp: r, name: "resistance".into(), value: Value::Choice("10k".into()) });
    assert!(a.patch.is_empty());
    assert_eq!(b.d.rev(), rev);
}

//! Consistency of the library data (contract §2, §2.2, §4) against the rules it must satisfy.

mod common;

use std::collections::BTreeSet;

use common::{add, doc, pref};
use n7rt_circuit::{geom, kind_def, library, Dir, Kind, Lit, Pt, Rect, Rot, ValueType, SAMPLE_RATES_HZ};

const ALL: [Kind; 25] = [
    Kind::DcSupply,
    Kind::Ground,
    Kind::Resistor,
    Kind::Capacitor,
    Kind::Potentiometer,
    Kind::PushButton,
    Kind::Switch,
    Kind::AdcInput,
    Kind::EcuStm32,
    Kind::CanTransceiver,
    Kind::CanBus,
    Kind::Termination,
    Kind::EcuApps,
    Kind::Mlx90377,
    Kind::Junction,
    Kind::EcuWsf, Kind::EcuSas, Kind::EcuBps, Kind::Ats682Lsh, Kind::Tle5014,
    Kind::Pss260, Kind::PressureSwitch, Kind::FrontAxle, Kind::SteeringColumn, Kind::BrakeHydraulics,
];

/// The lead of a pin: exactly 1 unit from the body edge, outside it, in its direction.
fn lead_ok(body: Rect, pos: Pt, dir: Dir) -> bool {
    let inside_x = body.x0 <= pos.x && pos.x <= body.x1;
    let inside_y = body.y0 <= pos.y && pos.y <= body.y1;
    match dir {
        Dir::L => pos.x == body.x0 - 1 && inside_y,
        Dir::R => pos.x == body.x1 + 1 && inside_y,
        Dir::U => pos.y == body.y0 - 1 && inside_x,
        Dir::D => pos.y == body.y1 + 1 && inside_x,
    }
}

#[test]
fn every_kind_once_in_palette_order() {
    let kinds: Vec<Kind> = library().iter().map(|d| d.kind).collect();
    assert_eq!(kinds, ALL.to_vec());
    for k in ALL {
        assert_eq!(kind_def(k).kind, k);
    }
}

#[test]
fn every_pin_is_one_unit_outside_its_body() {
    for def in library().iter().filter(|d| d.kind != Kind::Junction) {
        let b = def.body;
        assert!(b.x0 < b.x1 && b.y0 < b.y1, "{:?}: empty body", def.kind);
        for p in def.pins {
            assert!(lead_ok(b, p.pos, p.dir), "{:?}.{} at {:?} dir {:?}: lead is not 1 unit from body {:?}", def.kind, p.id, p.pos, p.dir, b);
        }
    }
    let j = kind_def(Kind::Junction);
    assert_eq!(j.body, Rect { x0: 0, y0: 0, x1: 0, y1: 0 });
    assert_eq!(j.pins.len(), 1);
    assert_eq!((j.pins[0].id, j.pins[0].pos), ("j", Pt { x: 0, y: 0 }));
    assert!(!j.placeable && j.params.is_empty() && j.states.is_empty());
}

#[test]
fn leads_stay_outside_the_body_in_every_rotation() {
    // World geometry from the model, in the four rotations: the same rule on the rotated body.
    for def in library().iter().filter(|d| d.kind != Kind::Junction) {
        for rot in [Rot::R0, Rot::R90, Rot::R180, Rot::R270] {
            let mut d = doc();
            let id = add(&mut d, def.kind, 100, -50, rot);
            let c = d.circuit();
            let body = c.body_rect(id).unwrap();
            for p in def.pins {
                let r = pref(id, p.id);
                let pos = c.pin_pos(&r).unwrap();
                let dir = c.pin_dir(&r).unwrap();
                assert_eq!(pos, Pt { x: 100 + geom::rotate(p.pos, rot).x, y: -50 + geom::rotate(p.pos, rot).y });
                assert_eq!(dir, geom::rotate_dir(p.dir, rot));
                assert!(lead_ok(body, pos, dir), "{:?} {:?} {}", def.kind, rot, p.id);
            }
        }
    }
}

#[test]
fn ids_are_unique_and_pins_distinct() {
    for def in library() {
        let pins: BTreeSet<&str> = def.pins.iter().map(|p| p.id).collect();
        assert_eq!(pins.len(), def.pins.len(), "{:?}: duplicate pin id", def.kind);
        let positions: BTreeSet<Pt> = def.pins.iter().map(|p| p.pos).collect();
        assert_eq!(positions.len(), def.pins.len(), "{:?}: two pins on one point", def.kind);
        let params: BTreeSet<&str> = def.params.iter().map(|p| p.name).collect();
        assert_eq!(params.len(), def.params.len(), "{:?}: duplicate parameter", def.kind);
        let states: BTreeSet<&str> = def.states.iter().map(|s| s.name).collect();
        assert_eq!(states.len(), def.states.len(), "{:?}: duplicate state", def.kind);
        assert!(params.is_disjoint(&states), "{:?}: a name is both parameter and state", def.kind);
        for p in def.pins {
            assert!(p.id.is_ascii() && !p.id.is_empty() && !p.short.is_empty() && !p.name.is_empty());
        }
    }
}

#[test]
fn defaults_are_within_their_ranges() {
    for def in library() {
        for p in def.params {
            assert!(p.min <= p.max, "{:?}.{}", def.kind, p.name);
            match (p.vtype, p.default) {
                (ValueType::Number, Lit::Num(x)) => assert!(x.is_finite() && p.min <= x && x <= p.max, "{:?}.{}", def.kind, p.name),
                (ValueType::NumberOrInf, Lit::Num(x)) => {
                    assert!(x == f64::INFINITY || (p.min <= x && x <= p.max), "{:?}.{}", def.kind, p.name);
                    assert!(p.min.is_finite() && p.max.is_finite());
                }
                (ValueType::Int, Lit::Int(i)) => {
                    assert!(p.min <= i as f64 && i as f64 <= p.max, "{:?}.{}", def.kind, p.name);
                    assert_eq!(p.min.fract(), 0.0);
                    assert_eq!(p.max.fract(), 0.0);
                }
                (ValueType::Bool, Lit::Bool(_)) => {}
                (ValueType::Choice, Lit::Choice(c)) => {
                    assert!(p.choices.len() >= 2 && p.choices.contains(&c), "{:?}.{}", def.kind, p.name);
                }
                (t, d) => panic!("{:?}.{}: default {d:?} does not match type {t:?}", def.kind, p.name),
            }
            if !p.editable && !p.hidden {
                assert_eq!((p.min, p.max), (p.default_num(), p.default_num()), "{:?}.{}: a locked value is fixed", def.kind, p.name);
            }
        }
        for s in def.states {
            match (s.vtype, s.default) {
                (ValueType::Bool, Lit::Bool(_)) => {}
                (ValueType::Number, Lit::Num(x)) => assert!(s.min <= x && x <= s.max),
                (t, d) => panic!("{:?}.{}: state default {d:?} vs {t:?}", def.kind, s.name),
            }
        }
    }
}

trait DefaultNum {
    fn default_num(&self) -> f64;
}

impl DefaultNum for n7rt_circuit::ParamDef {
    fn default_num(&self) -> f64 {
        match self.default {
            Lit::Num(x) => x,
            Lit::Int(i) => i as f64,
            _ => f64::NAN,
        }
    }
}

#[test]
fn units_follow_component_dimensions_and_ohm_is_u03a9() {
    const SI: [&str; 7] = ["V", "A", "\u{3a9}", "F", "W", "s", "Hz"];
    for def in library() {
        for p in def.params {
            let mechanical = match (def.kind, p.name) {
                (Kind::Ats682Lsh, "air_gap_mm") => Some("mm"),
                (Kind::Ats682Lsh, "tolerance_corner") | (Kind::BrakeHydraulics, "beta") => Some(""),
                (Kind::BrakeHydraulics, "min_pressure_bar" | "max_pressure_bar") => Some("bar"),
                (Kind::SteeringColumn, "min_angle_deg" | "max_angle_deg") => Some("°"),
                (Kind::SteeringColumn, "max_rate_deg_per_s") => Some("°/s"),
                _ => None,
            };
            if let Some(unit) = mechanical { assert_eq!(p.unit, unit); continue; }
            match p.vtype {
                // A hidden value in %VDD (transfer function) is dimensionless; it is never shown.
                ValueType::Number if p.hidden => assert!(SI.contains(&p.unit) || p.unit.is_empty(), "{:?}.{}", def.kind, p.name),
                ValueType::Number | ValueType::NumberOrInf => assert!(SI.contains(&p.unit), "{:?}.{}: unit {:?}", def.kind, p.name, p.unit),
                ValueType::Int => assert!(["", "bit", "Hz"].contains(&p.unit), "{:?}.{}", def.kind, p.name),
                ValueType::Bool | ValueType::Choice => assert_eq!(p.unit, ""),
            }
        }
        // The ohm sign U+2126 never appears: Ω is always the Greek capital omega.
        let texts = [def.name_fr, def.sources_fr]
            .into_iter()
            .chain(def.params.iter().flat_map(|p| [p.label_fr, p.unit]))
            .chain(def.pins.iter().flat_map(|p| [p.name, p.short]));
        for t in texts {
            assert!(!t.contains('\u{2126}'), "{:?}: U+2126 in {t:?}", def.kind);
        }
    }
}

#[test]
fn special_parameters_follow_contract_4() {
    let adc = kind_def(Kind::AdcInput);
    let rate = adc.params.iter().find(|p| p.name == "sample_rate").unwrap();
    assert_eq!(rate.default, Lit::Int(1000));
    assert!(SAMPLE_RATES_HZ.iter().all(|r| 1000 % r == 0));
    assert_eq!(SAMPLE_RATES_HZ.len(), (1..=1000).filter(|r| 1000 % r == 0).count());
    let bits = adc.params.iter().find(|p| p.name == "bits").unwrap();
    assert_eq!((bits.vtype, bits.min, bits.max), (ValueType::Int, 8.0, 16.0));
    let r_in = adc.params.iter().find(|p| p.name == "r_in").unwrap();
    assert_eq!((r_in.vtype, r_in.default), (ValueType::NumberOrInf, Lit::Num(f64::INFINITY)));
    let ecu = kind_def(Kind::EcuStm32);
    let adc_bits = ecu.params.iter().find(|p| p.name == "adc_bits").unwrap();
    assert_eq!(adc_bits.choices, &["8", "10", "12", "14", "16"]);
    let locked: Vec<(Kind, &str)> = library()
        .iter()
        .flat_map(|d| d.params.iter().filter(|p| !p.editable && !p.hidden).map(move |p| (d.kind, p.name)))
        .collect();
    assert_eq!(locked, vec![(Kind::Termination, "resistance"), (Kind::Termination, "power_rating")]);
}

#[test]
fn designators_and_actuators() {
    for def in library() {
        let no_designator = matches!(def.kind, Kind::Ground | Kind::Junction);
        assert_eq!(def.prefix.is_empty(), no_designator, "{:?}", def.kind);
        assert!(def.prefix.chars().all(|c| c.is_ascii_uppercase()));
        assert_eq!(def.placeable, def.kind != Kind::Junction);
        for a in def.actuators {
            let (z, b) = (a.zone, def.body);
            assert!(b.x0 <= z.x0 && z.x1 <= b.x1 && b.y0 <= z.y0 && z.y1 <= b.y1, "{:?}: actuator outside the body", def.kind);
        }
    }
}

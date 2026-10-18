//! Every edit of contract §3.2 with the geometry of §3.4, every refusal code and ambiguity sub-code, junctions,
//! repairs, rotations, nets and names. Each scenario is minimal and its expected geometry is derived by hand from
//! the catalog (resistor pins at ±3, R90 maps (x, y) to (−y, x)).

mod common;

use common::*;
use n7rt_circuit::{
    Ambiguity, CompId, Component, Document, Edit, End, ItemRef, Kind, RefusalCode, Rot, Value, Wire, WireId,
};

/// R1 at (0,0) and R2 at (12,0), joined by W1 = R1.p2 (3,0) → R2.p1 (9,0). Ids: R1 = 1, R2 = 2, W1 = 3.
fn two_resistors() -> (Document, CompId, CompId, WireId) {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
    let w1 = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None);
    assert_eq!((r1, r2, w1), (CompId(1), CompId(2), WireId(3)));
    assert_eq!(path(&d, w1), pts(&[(3, 0), (9, 0)]));
    (d, r1, r2, w1)
}

fn ambiguous(d: &mut Document, e: Edit, sub: Ambiguity) -> n7rt_circuit::Refusal {
    let r = refused_code(d, e, RefusalCode::Ambiguous);
    assert_eq!(r.ambiguity, Some(sub), "{}", r.message_fr);
    r
}

fn comp(d: &Document, id: CompId) -> &Component {
    &d.circuit().components()[&id]
}

fn wire_of(d: &Document, id: WireId) -> &Wire {
    &d.circuit().wires()[&id]
}

fn junctions(d: &Document) -> Vec<CompId> {
    d.circuit().components().values().filter(|c| c.kind == Kind::Junction).map(|c| c.id).collect()
}

fn checked(d: &mut Document, e: Edit) -> n7rt_circuit::Applied {
    let before = d.circuit().clone();
    let a = edit(d, e);
    assert_patch_exact(&before, d.circuit(), &a.patch);
    assert_valid(d);
    a
}

// ------------------------------------------------------------------------------------------------ add, labels

#[test]
fn add_component_with_defaults_labels_and_overrides() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let c = comp(&d, r1);
    assert_eq!(c.label, "R1");
    assert_eq!(c.params["resistance"], Value::Num(1e4));
    assert_eq!(c.params["power_rating"], Value::Num(0.25));
    assert_eq!(c.params.len(), 2);
    let r2 = add(&mut d, Kind::Resistor, 0, 10, Rot::R0);
    assert_eq!(comp(&d, r2).label, "R2");
    let g = add(&mut d, Kind::Ground, 20, 0, Rot::R0);
    assert_eq!(comp(&d, g).label, "");
    let u1 = add(&mut d, Kind::EcuStm32, 40, 0, Rot::R0);
    let u2 = add(&mut d, Kind::CanTransceiver, 70, 0, Rot::R0);
    assert_eq!((comp(&d, u1).label.as_str(), comp(&d, u2).label.as_str()), ("U1", "U2"));
    let rv = add(&mut d, Kind::Potentiometer, 0, 30, Rot::R0);
    assert_eq!(comp(&d, rv).label, "RV1");
    assert_eq!(d.circuit().state(rv).unwrap()["position"], Value::Num(0.5));
    // Smallest free number.
    checked(&mut d, Edit::Delete { components: vec![r1], wires: vec![] });
    let r3 = add(&mut d, Kind::Resistor, 0, 50, Rot::R0);
    assert_eq!(comp(&d, r3).label, "R1");
    // Overrides, text included; explicit label.
    let a = checked(
        &mut d,
        Edit::AddComponent {
            kind: Kind::Resistor,
            pos: pt(0, 70),
            rot: Rot::R90,
            params: [("resistance".to_string(), Value::Choice("4k7".into()))].into(),
            label: Some("Rload".into()),
        },
    );
    let (id, _, after) = &a.patch.components[0];
    let after = after.as_ref().unwrap();
    assert_eq!((after.label.as_str(), &after.params["resistance"], after.rot), ("Rload", &Value::Num(4700.0), Rot::R90));
    assert!(a.topology_changed && a.electrical_changed);
    assert_eq!(*id, CompId(d.circuit().next_id() - 1));
}

#[test]
fn add_component_refusals() {
    let (mut d, _, _, _) = two_resistors();
    let add_e = |kind: Kind, x: i32, y: i32, label: Option<&str>| Edit::AddComponent {
        kind,
        pos: pt(x, y),
        rot: Rot::R0,
        params: Default::default(),
        label: label.map(str::to_string),
    };
    let r = refused_code(&mut d, add_e(Kind::Junction, 50, 50, None), RefusalCode::UnknownKind);
    assert_eq!(r.message_fr, "Une jonction ne se place pas seule : reliez un fil au milieu d'un autre fil");
    let r = refused_code(&mut d, add_e(Kind::Resistor, 50, 50, Some("R2")), RefusalCode::LabelDuplicate);
    assert_eq!(r.message_fr, "Le d\u{e9}signateur R2 est d\u{e9}j\u{e0} utilis\u{e9}");
    for bad in ["1R", "", "R-1", "R\u{e9}", "Rabcdefghijklmnop"] {
        refused_code(&mut d, add_e(Kind::Resistor, 50, 50, Some(bad)), RefusalCode::LabelInvalid);
    }
    edit(&mut d, add_e(Kind::Resistor, 50, 50, Some("Rabcdefghijklmno")));
    let r = refused_code(&mut d, add_e(Kind::Ground, 70, 50, Some("G1")), RefusalCode::LabelInvalid);
    assert_eq!(r.message_fr, "Masse : pas de d\u{e9}signateur");
    // A resistor at x = 9997 has its pin p2 on the limit; one more unit is outside.
    edit(&mut d, add_e(Kind::Resistor, 9997, 0, None));
    let r = refused_code(&mut d, add_e(Kind::Resistor, 9998, 20, None), RefusalCode::OutOfBounds);
    assert_eq!(r.at, Some(pt(10001, 20)));
    let e = Edit::AddComponent {
        kind: Kind::Resistor,
        pos: pt(60, 60),
        rot: Rot::R0,
        params: [("resistance".to_string(), Value::Num(0.0))].into(),
        label: None,
    };
    refused_code(&mut d, e, RefusalCode::ParamOutOfRange);
    let e = Edit::AddComponent {
        kind: Kind::Resistor,
        pos: pt(60, 60),
        rot: Rot::R0,
        params: [("tolerance".to_string(), Value::Num(0.05))].into(),
        label: None,
    };
    refused_code(&mut d, e, RefusalCode::ParamUnknown);
    // A locked parameter may be given only with its fixed value.
    let locked = |v: f64| Edit::AddComponent {
        kind: Kind::Termination,
        pos: pt(80, 80),
        rot: Rot::R0,
        params: [("resistance".to_string(), Value::Num(v))].into(),
        label: None,
    };
    refused_code(&mut d, locked(100.0), RefusalCode::ParamLocked);
    edit(&mut d, locked(120.0));
}

// ------------------------------------------------------------------------------------------------ ambiguities

#[test]
fn ambiguity_pin_on_pin() {
    let (mut d, _, _, _) = two_resistors();
    // A resistor at (6, 10) R180 would put its p2 on (3, 10): free. At (6, 0) its p1 is (3, 0) = R1.p2.
    let r = ambiguous(&mut d, Edit::AddComponent { kind: Kind::Resistor, pos: pt(6, 0), rot: Rot::R0, params: Default::default(), label: None }, Ambiguity::PinOnPin);
    assert_eq!(r.at, Some(pt(3, 0)));
    assert!(r.items.contains(&ItemRef::Pin { comp: CompId(1), pin: "p2".into() }));
}

#[test]
fn ambiguity_pin_on_wire_for_a_component_edit() {
    let (mut d, _, _, w1) = two_resistors();
    // A ground pin at (6, 0) lies inside W1 (3,0)→(9,0).
    let r = ambiguous(&mut d, Edit::AddComponent { kind: Kind::Ground, pos: pt(6, 0), rot: Rot::R0, params: Default::default(), label: None }, Ambiguity::PinOnWire);
    assert_eq!(r.at, Some(pt(6, 0)));
    assert!(r.items.contains(&ItemRef::Wire { id: w1 }));
    assert_eq!(r.message_fr, "La broche GND toucherait un fil sans y \u{ea}tre reli\u{e9}e");
    // Moving a component onto a wire is a component edit too.
    let g = add(&mut d, Kind::Ground, 6, 10, Rot::R0);
    ambiguous(&mut d, Edit::Move { components: vec![g], wires: vec![], delta: pt(0, -10) }, Ambiguity::PinOnWire);
}

#[test]
fn ambiguity_wire_over_pin_for_a_wire_edit() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    // R2 vertical: p1 at (6, 0) pointing up, body (5,1)–(7,5).
    let _r2 = add(&mut d, Kind::Resistor, 6, 3, Rot::R90);
    let r3 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
    let r = ambiguous(&mut d, Edit::AddWire { a: pin(r1, "p2"), b: pin(r3, "p1"), bends: Some(vec![]) }, Ambiguity::WireOverPin);
    assert_eq!(r.message_fr, "Ce fil passerait sur la broche R2.1 sans s'y relier");
    assert_eq!(r.at, Some(pt(6, 0)));
}

#[test]
fn ambiguity_collinear_overlap_wire_wire_and_wire_lead() {
    let (mut d, r1, _, w1) = two_resistors();
    // R5 vertical below: p1 at (6, 5).
    let r5 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    let r = ambiguous(&mut d, Edit::AddWire { a: pin(r1, "p2"), b: pin(r5, "p1"), bends: Some(pts(&[(6, 0)])) }, Ambiguity::CollinearOverlap);
    assert!(r.items.contains(&ItemRef::Wire { id: w1 }));
    assert_eq!(r.at, Some(pt(3, 0)));
    // Back over its own lead into R1: overlap with the lead first (the body comes later in the rule order).
    let r = ambiguous(
        &mut d,
        Edit::AddWire { a: pin(r1, "p2"), b: pin(r5, "p1"), bends: Some(pts(&[(2, 0), (2, 3), (6, 3)])) },
        Ambiguity::CollinearOverlap,
    );
    assert_eq!(r.message_fr, "Un fil recouvrirait la patte de la broche R1.2");
    // Sharing an end point is not an overlap: a second wire from R1.p2 going down is fine.
    let w = wire(&mut d, pin(r1, "p2"), pin(r5, "p1"), Some(pts(&[(3, 3), (6, 3)])));
    assert_eq!(path(&d, w), pts(&[(3, 0), (3, 3), (6, 3), (6, 5)]));
    assert_valid(&d);
}

#[test]
fn ambiguity_self_overlap() {
    let (mut d, _, _, w1) = two_resistors();
    let bends = pts(&[(7, 0), (7, 2), (5, 2), (5, 0), (6, 0), (6, -2), (9, -2)]);
    let r = ambiguous(&mut d, Edit::SetBends { wire: w1, bends }, Ambiguity::SelfOverlap);
    assert_eq!(r.items, vec![ItemRef::Wire { id: w1 }]);
    // A bend of the wire on its own segment: touching itself.
    let bends = pts(&[(6, 0), (6, -2), (8, -2), (8, 2), (5, 2), (5, 0)]);
    ambiguous(&mut d, Edit::SetBends { wire: w1, bends }, Ambiguity::SelfOverlap);
    // A clean perpendicular crossing of the wire with itself is not a touch: (3,0)→(7,0) is crossed at (5,0) by
    // (5,3)→(5,−2), like any crossing of two interiors.
    let bends = pts(&[(7, 0), (7, 3), (5, 3), (5, -2), (9, -2)]);
    checked(&mut d, Edit::SetBends { wire: w1, bends: bends.clone() });
    assert_eq!(wire_of(&d, w1).bends, bends);
}

#[test]
fn ambiguity_bend_on_bend() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0); // p2 (3,0)
    let r5 = add(&mut d, Kind::Resistor, 9, 7, Rot::R0); // p1 (6,7)
    let r6 = add(&mut d, Kind::Resistor, 6, -8, Rot::R90); // p2 (6,−5)
    let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0); // p1 (9,0)
    wire(&mut d, pin(r1, "p2"), pin(r5, "p1"), Some(pts(&[(6, 0)])));
    // Corner on corner at (6, 0): the two L touch only there and look joined.
    let r = ambiguous(&mut d, Edit::AddWire { a: pin(r6, "p2"), b: pin(r2, "p1"), bends: Some(pts(&[(6, 0)])) }, Ambiguity::BendOnWire);
    assert_eq!(r.at, Some(pt(6, 0)));
}

#[test]
fn ambiguity_wire_through_body() {
    let (mut d, r1, r2, _) = two_resistors();
    // A body over an existing wire: the top edge of R3 (4,0)–(8,2) touches W1 at y = 0.
    let r = ambiguous(&mut d, Edit::AddComponent { kind: Kind::Resistor, pos: pt(6, 1), rot: Rot::R0, params: Default::default(), label: None }, Ambiguity::WireThroughBody);
    assert!(r.items.contains(&ItemRef::Component { id: CompId(4) }));
    // A wire across a body.
    let r4 = add(&mut d, Kind::Resistor, 6, -10, Rot::R0); // body (4,−11)–(8,−9), pins (3,−10), (9,−10)
    let r = ambiguous(&mut d, Edit::SetBends { wire: WireId(3), bends: pts(&[(3, -10), (9, -10)]) }, Ambiguity::PinOnWire.wire_edit());
    assert!(r.items.contains(&ItemRef::Pin { comp: r4, pin: "p1".into() }));
    // Up through the body at x = 5, around above it and back down at x = 11 (clear of the pins at x = 3 and 9).
    let bends = pts(&[(5, 0), (5, -12), (11, -12), (11, -3), (9, -3)]);
    let r = ambiguous(&mut d, Edit::SetBends { wire: WireId(3), bends }, Ambiguity::WireThroughBody);
    assert_eq!(r.message_fr, "Ce fil traverserait le corps de R3");
    assert_eq!(r.items, vec![ItemRef::Wire { id: WireId(3) }, ItemRef::Component { id: r4 }]);
    // The rectangle is closed: running along its bottom edge y = −9 crosses it; one unit below is clear.
    ambiguous(&mut d, Edit::SetBends { wire: WireId(3), bends: pts(&[(5, 0), (5, -9), (9, -9)]) }, Ambiguity::WireThroughBody);
    checked(&mut d, Edit::SetBends { wire: WireId(3), bends: pts(&[(5, 0), (5, -8), (9, -8)]) });
    let _ = (r1, r2);
}

trait WireEdit {
    fn wire_edit(self) -> Ambiguity;
}

impl WireEdit for Ambiguity {
    fn wire_edit(self) -> Ambiguity {
        match self {
            Ambiguity::PinOnWire => Ambiguity::WireOverPin,
            a => a,
        }
    }
}

// ------------------------------------------------------------------------------------------------ wires

#[test]
fn add_wire_autoroute_explicit_bends_and_refusals() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0); // p2 (3,0) → right
    let r3 = add(&mut d, Kind::Resistor, 10, 6, Rot::R90); // p1 (10,3) ↑
    // Autoroute: straight impossible, L via (b.x, a.y) = (10, 0) arrives on p1 from above.
    let w = wire(&mut d, pin(r1, "p2"), pin(r3, "p1"), None);
    assert_eq!(path(&d, w), pts(&[(3, 0), (10, 0), (10, 3)]));
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![w] });
    // Explicit bends are canonicalized.
    let w = wire(&mut d, pin(r1, "p2"), pin(r3, "p1"), Some(pts(&[(5, 0), (7, 0), (7, 1), (7, 3)])));
    assert_eq!(wire_of(&d, w).bends, pts(&[(7, 0), (7, 3)]));
    assert_eq!(path(&d, w), pts(&[(3, 0), (7, 0), (7, 3), (10, 3)]));
    // Refusals.
    let r = refused_code(&mut d, Edit::AddWire { a: pin(r1, "p1"), b: pin(r1, "p1"), bends: None }, RefusalCode::SamePin);
    assert_eq!(r.message_fr, "Un fil ne peut pas relier R1.1 \u{e0} elle-m\u{ea}me");
    let r = refused_code(&mut d, Edit::AddWire { a: pin(r3, "p1"), b: pin(r1, "p2"), bends: None }, RefusalCode::DuplicateWire);
    assert_eq!(r.message_fr, "R2.1 et R1.2 sont d\u{e9}j\u{e0} reli\u{e9}es par un fil");
    let r = refused_code(&mut d, Edit::AddWire { a: pin(r1, "p3"), b: pin(r3, "p2"), bends: None }, RefusalCode::UnknownPin);
    assert_eq!(r.message_fr, "R1 n'a pas de broche \u{ab}\u{a0}p3\u{a0}\u{bb}");
    refused_code(&mut d, Edit::AddWire { a: pin(CompId(99), "p1"), b: pin(r3, "p2"), bends: None }, RefusalCode::UnknownComponent);
    refused_code(&mut d, Edit::AddWire { a: on_wire(WireId(99), 0, 0), b: pin(r3, "p2"), bends: None }, RefusalCode::UnknownWire);
    refused_code(&mut d, Edit::AddWire { a: pin(r1, "p1"), b: pin(r3, "p2"), bends: Some(vec![]) }, RefusalCode::NonOrthogonal);
    let r = refused_code(&mut d, Edit::AddWire { a: pin(r1, "p1"), b: pin(r3, "p2"), bends: Some(pts(&[(-5, 4)])) }, RefusalCode::NonOrthogonal);
    assert_eq!(r.at, Some(pt(-3, 0)));
    refused_code(&mut d, Edit::AddWire { a: pin(r1, "p1"), b: pin(r3, "p2"), bends: Some(pts(&[(-3, 10009), (10, 10009)])) }, RefusalCode::OutOfBounds);
}

#[test]
fn no_route_when_every_candidate_is_invalid() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    // From p2 to p1 of the same resistor every candidate collapses onto the straight line through its body.
    let r = refused_code(&mut d, Edit::AddWire { a: pin(r1, "p2"), b: pin(r1, "p1"), bends: None }, RefusalCode::NoRoute);
    assert_eq!(r.message_fr, "Aucun trac\u{e9} libre entre R1.2 et R1.1 : dessinez les coudes \u{e0} la main");
    assert!(r.at.is_some());
    // By hand, around the body, it is accepted.
    wire(&mut d, pin(r1, "p2"), pin(r1, "p1"), Some(pts(&[(5, 0), (5, -3), (-5, -3), (-5, 0)])));
    assert_valid(&d);
}

#[test]
fn junction_creation_splits_the_wire_with_the_fixed_id_order() {
    let (mut d, r1, r2, w1) = two_resistors();
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90); // id 4, p1 (6,5) ↑
    let before = d.circuit().clone();
    let a = edit(&mut d, Edit::AddWire { a: on_wire(w1, 6, 0), b: pin(r3, "p1"), bends: None });
    assert_patch_exact(&before, d.circuit(), &a.patch);
    assert_valid(&d);
    // a: junction 5, second half 6; the new wire last: 7.
    let j = CompId(5);
    assert_eq!(junctions(&d), vec![j]);
    assert_eq!(comp(&d, j).pos, pt(6, 0));
    assert_eq!(wire_of(&d, w1), &Wire { id: w1, a: pref(r1, "p2"), b: pref(j, "j"), bends: vec![] });
    assert_eq!(wire_of(&d, WireId(6)), &Wire { id: WireId(6), a: pref(j, "j"), b: pref(r2, "p1"), bends: vec![] });
    assert_eq!(wire_of(&d, WireId(7)), &Wire { id: WireId(7), a: pref(j, "j"), b: pref(r3, "p1"), bends: vec![] });
    assert_eq!(d.circuit().next_id(), 8);
    assert_eq!(a.patch.components.iter().map(|c| c.0).collect::<Vec<_>>(), vec![j]);
    assert_eq!(a.patch.wires.iter().map(|w| w.0).collect::<Vec<_>>(), vec![w1, WireId(6), WireId(7)]);
    assert!(a.topology_changed);
    // The net now holds the junction, and is named after its smallest real pin.
    let nets = d.circuit().netlist();
    let net = nets.net_of(&pref(j, "j")).unwrap();
    assert_eq!(net.name, "R1.2");
    assert_eq!(net.pins, vec![pref(r1, "p2"), pref(r2, "p1"), pref(r3, "p1"), pref(j, "j")]);
    assert_eq!(net.wires, vec![w1, WireId(6), WireId(7)]);
}

#[test]
fn junctions_at_both_ends_order_a_then_b_then_the_wire() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
    let r3 = add(&mut d, Kind::Resistor, 0, 10, Rot::R0);
    let r4 = add(&mut d, Kind::Resistor, 12, 10, Rot::R0);
    let wa = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None); // 5
    let wb = wire(&mut d, pin(r3, "p2"), pin(r4, "p1"), None); // 6
    let a = checked(&mut d, Edit::AddWire { a: on_wire(wa, 6, 0), b: on_wire(wb, 5, 10), bends: None });
    // Ja = 7, half of wa = 8, Jb = 9, half of wb = 10, new wire 11 (Z from (6,0) to (5,10)).
    assert_eq!(junctions(&d), vec![CompId(7), CompId(9)]);
    assert_eq!(wire_of(&d, WireId(8)).a, pref(CompId(7), "j"));
    assert_eq!(wire_of(&d, WireId(10)).a, pref(CompId(9), "j"));
    let new = wire_of(&d, WireId(11));
    assert_eq!((new.a.clone(), new.b.clone()), (pref(CompId(7), "j"), pref(CompId(9), "j")));
    assert_eq!(path(&d, WireId(11)), pts(&[(6, 0), (6, 5), (5, 5), (5, 10)]));
    assert_eq!(a.patch.wires.len(), 5);
}

#[test]
fn on_wire_endpoint_rules() {
    let (mut d, r1, _, w1) = two_resistors();
    let r3 = add(&mut d, Kind::Resistor, 3, 8, Rot::R90); // p1 (3,5)
    // `at` on an end of the wire is that end's pin: no junction.
    let w = wire(&mut d, on_wire(w1, 3, 0), pin(r3, "p1"), None);
    assert_eq!(wire_of(&d, w).a, pref(r1, "p2"));
    assert!(junctions(&d).is_empty());
    // Off the wire.
    let r = refused_code(&mut d, Edit::AddWire { a: on_wire(w1, 6, 1), b: pin(r3, "p2"), bends: None }, RefusalCode::EndpointNotOnWire);
    assert_eq!(r.message_fr, "Le point (6, 1) n'est pas sur ce fil");
    // Both ends on one wire.
    let r = refused_code(&mut d, Edit::AddWire { a: on_wire(w1, 5, 0), b: on_wire(w1, 7, 0), bends: None }, RefusalCode::SameWire);
    assert_eq!(r.message_fr, "Les deux extr\u{e9}mit\u{e9}s sont sur le m\u{ea}me fil");
    // A split that recreates an existing pair is a duplicate.
    refused_code(&mut d, Edit::AddWire { a: on_wire(w1, 6, 0), b: pin(CompId(2), "p1"), bends: None }, RefusalCode::DuplicateWire);
}

#[test]
fn four_way_junctions_are_refused() {
    let (mut d, _, _, w1) = two_resistors();
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90); // p1 (6,5)
    wire(&mut d, on_wire(w1, 6, 0), pin(r3, "p1"), None); // J = 5 of degree 3
    let r4 = add(&mut d, Kind::Resistor, 6, -8, Rot::R90); // p2 (6,−5)
    // A fourth wire on the junction, by its pin or by `at` on its position (an end of W1).
    let r = refused_code(&mut d, Edit::AddWire { a: pin(CompId(5), "j"), b: pin(r4, "p2"), bends: None }, RefusalCode::FourWayJunction);
    assert_eq!(r.message_fr, "Jonction \u{e0} 4 branches interdite : d\u{e9}calez en deux T");
    refused_code(&mut d, Edit::AddWire { a: on_wire(w1, 6, 0), b: pin(r4, "p2"), bends: None }, RefusalCode::FourWayJunction);
    // A junction at the crossing of two wires.
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r2 = add(&mut d, Kind::Resistor, 12, 0, Rot::R0);
    let r3 = add(&mut d, Kind::Resistor, 6, -8, Rot::R90); // p2 (6,−5)
    let r4 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90); // p1 (6,5)
    let r5 = add(&mut d, Kind::Resistor, 20, 10, Rot::R0);
    let h = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None);
    let v = wire(&mut d, pin(r3, "p2"), pin(r4, "p1"), None); // crosses h at (6,0): allowed, nothing joined
    assert_valid(&d);
    assert_ne!(d.circuit().netlist().net_of(&pref(r1, "p2")), d.circuit().netlist().net_of(&pref(r3, "p2")));
    let r = refused_code(&mut d, Edit::AddWire { a: on_wire(h, 6, 0), b: pin(r5, "p1"), bends: None }, RefusalCode::FourWayJunction);
    assert_eq!(r.items, vec![ItemRef::Wire { id: h }, ItemRef::Wire { id: v }]);
}

#[test]
fn set_bends_reshapes_and_canonicalizes() {
    let (mut d, _, _, w1) = two_resistors();
    let a = checked(&mut d, Edit::SetBends { wire: w1, bends: pts(&[(3, -2), (3, -4), (6, -4), (9, -4)]) });
    assert_eq!(wire_of(&d, w1).bends, pts(&[(3, -4), (9, -4)]));
    assert!(!a.topology_changed && !a.electrical_changed);
    // Same shape again: empty patch, no revision.
    let rev = d.rev();
    let a = edit(&mut d, Edit::SetBends { wire: w1, bends: pts(&[(3, -4), (9, -4)]) });
    assert!(a.patch.is_empty());
    assert_eq!(d.rev(), rev);
    refused_code(&mut d, Edit::SetBends { wire: w1, bends: pts(&[(5, -3)]) }, RefusalCode::NonOrthogonal);
    refused_code(&mut d, Edit::SetBends { wire: w1, bends: pts(&[(3, -20000), (9, -20000)]) }, RefusalCode::OutOfBounds);
    refused_code(&mut d, Edit::SetBends { wire: WireId(42), bends: vec![] }, RefusalCode::UnknownWire);
}

#[test]
fn move_wire_end_reattaches_and_cleans_the_abandoned_junction() {
    let (mut d, r1, r2, w1) = two_resistors();
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90); // p1 (6,5)
    let r4 = add(&mut d, Kind::Resistor, 20, 8, Rot::R90); // p1 (20,5)
    wire(&mut d, on_wire(w1, 6, 0), pin(r3, "p1"), None); // J 6, half 7, branch 8
    let j = CompId(6);
    // Reattach the branch's junction end to R4.p1: J keeps two wires and merges them into W1.
    let a = checked(&mut d, Edit::MoveWireEnd { wire: WireId(8), end: End::A, to: pin(r4, "p1"), bends: None });
    assert!(junctions(&d).is_empty());
    assert_eq!(wire_of(&d, w1), &Wire { id: w1, a: pref(r1, "p2"), b: pref(r2, "p1"), bends: vec![] });
    assert!(!d.circuit().wires().contains_key(&WireId(7)));
    let moved = wire_of(&d, WireId(8));
    assert_eq!((moved.a.clone(), moved.b.clone()), (pref(r4, "p1"), pref(r3, "p1")));
    // Autoroute from R4.p1 (20,5) to R3.p1 (6,5): aligned, the straight candidate is free.
    assert_eq!(path(&d, WireId(8)), pts(&[(20, 5), (6, 5)]));
    assert!(a.patch.components.iter().any(|(id, b, a)| *id == j && b.is_some() && a.is_none()));
    // Onto itself.
    refused_code(&mut d, Edit::MoveWireEnd { wire: w1, end: End::B, to: on_wire(w1, 5, 0), bends: None }, RefusalCode::SameWire);
    refused_code(&mut d, Edit::MoveWireEnd { wire: w1, end: End::B, to: pin(r1, "p2"), bends: None }, RefusalCode::SamePin);
    refused_code(&mut d, Edit::MoveWireEnd { wire: WireId(99), end: End::B, to: pin(r1, "p2"), bends: None }, RefusalCode::UnknownWire);
}

// ------------------------------------------------------------------------------------------------ cleanup

/// R1 —W1— J —H— R2 with a branch B from J down to R3.p1 (6,5). Ids: R1 1, R2 2, W1 3, R3 4, J 5, H 6, B 7.
fn tee() -> Document {
    let (mut d, _, _, w1) = two_resistors();
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    wire(&mut d, on_wire(w1, 6, 0), pin(r3, "p1"), None);
    d
}

#[test]
fn cleanup_degree_0_1_2() {
    // Degree 0: the three wires deleted at once.
    let mut d = tee();
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(3), WireId(6), WireId(7)] });
    assert!(junctions(&d).is_empty() && d.circuit().wires().is_empty());
    // Degree 1: the junction and its last wire go.
    let mut d = tee();
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(3), WireId(7)] });
    assert!(junctions(&d).is_empty() && d.circuit().wires().is_empty());
    // Degree 2: merged into the smallest wire id, path concatenated and canonical.
    let mut d = tee();
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(7)] });
    assert!(junctions(&d).is_empty());
    assert_eq!(d.circuit().wires().keys().copied().collect::<Vec<_>>(), vec![WireId(3)]);
    assert_eq!(wire_of(&d, WireId(3)), &Wire { id: WireId(3), a: pref(CompId(1), "p2"), b: pref(CompId(2), "p1"), bends: vec![] });
    // Degree 2 where the smallest wire starts on the junction: W1 (b = J) deleted, H (a = J) and B (a = J) merge into
    // H, oriented from H's far end: R2.p1 (9,0) → (6,0) → R3.p1 (6,5).
    let mut d = tee();
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(3)] });
    assert_eq!(wire_of(&d, WireId(6)), &Wire { id: WireId(6), a: pref(CompId(2), "p1"), b: pref(CompId(4), "p1"), bends: pts(&[(6, 0)]) });
    // A selected junction is ignored by delete: the cleanup decides.
    let mut d = tee();
    let a = checked(&mut d, Edit::Delete { components: vec![CompId(5)], wires: vec![] });
    assert!(a.patch.is_empty());
    assert_eq!(junctions(&d), vec![CompId(5)]);
}

#[test]
fn cleanup_skips_a_junction_whose_merge_would_duplicate_then_cascades() {
    let mut d = tee();
    // A second, direct wire R1.p2 → R2.p1 over the top (no duplicate: W1 now ends on J).
    let w8 = wire(&mut d, pin(CompId(1), "p2"), pin(CompId(2), "p1"), Some(pts(&[(3, -3), (9, -3)])));
    assert_eq!(w8, WireId(8));
    // Without its branch J has degree 2, but merging W1 and H would duplicate W8: J stays (legal, drawn as a bend).
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(7)] });
    assert_eq!(junctions(&d), vec![CompId(5)]);
    assert!(d.circuit().check().is_empty());
    // Once W8 is gone the merge is allowed.
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![w8] });
    assert!(junctions(&d).is_empty());
    assert_eq!(path(&d, WireId(3)), pts(&[(3, 0), (9, 0)]));
}

#[test]
fn splitting_the_duplicate_of_a_skipped_junction_lets_the_cleanup_merge_it() {
    let mut d = tee();
    let w8 = wire(&mut d, pin(CompId(1), "p2"), pin(CompId(2), "p1"), Some(pts(&[(3, -3), (9, -3)])));
    checked(&mut d, Edit::Delete { components: vec![], wires: vec![WireId(7)] });
    assert_eq!(junctions(&d), vec![CompId(5)]);
    // R6 (id 9) above W8; a branch from W8 at (6,−3) splits it: W8 no longer joins R1.p2 and R2.p1, so J5 may merge.
    let r6 = add(&mut d, Kind::Resistor, 6, -11, Rot::R90); // p2 (6,−8)
    let a = checked(&mut d, Edit::AddWire { a: on_wire(w8, 6, -3), b: pin(r6, "p2"), bends: None });
    assert_eq!(junctions(&d), vec![CompId(10)]);
    assert_eq!(wire_of(&d, WireId(3)), &Wire { id: WireId(3), a: pref(CompId(1), "p2"), b: pref(CompId(2), "p1"), bends: vec![] });
    assert!(a.patch.wires.iter().any(|(id, b, a)| *id == WireId(6) && b.is_some() && a.is_none()));
    assert_eq!(path(&d, w8), pts(&[(3, 0), (3, -3), (6, -3)]));
    // Undo restores the skipped junction exactly.
    d.undo().unwrap();
    assert_eq!(junctions(&d), vec![CompId(5)]);
    assert_valid(&d);
}

#[test]
fn cleanup_cascades_through_successive_merges() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0); // 1, p2 (3,0)
    let r2 = add(&mut d, Kind::Resistor, 16, 0, Rot::R0); // 2, p1 (13,0)
    let w1 = wire(&mut d, pin(r1, "p2"), pin(r2, "p1"), None); // 3
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90); // 4, p1 (6,5)
    let r4 = add(&mut d, Kind::Resistor, 10, 8, Rot::R90); // 5, p1 (10,5)
    wire(&mut d, on_wire(w1, 6, 0), pin(r3, "p1"), None); // J1 6, H1 7, B1 8
    wire(&mut d, on_wire(WireId(7), 10, 0), pin(r4, "p1"), None); // J2 9, H2 10, B2 11
    assert_eq!(junctions(&d), vec![CompId(6), CompId(9)]);
    // Deleting both branch components leaves J1 and J2 at degree 2: J1 merges first, then J2.
    checked(&mut d, Edit::Delete { components: vec![r3, r4], wires: vec![] });
    assert!(junctions(&d).is_empty());
    assert_eq!(d.circuit().wires().len(), 1);
    assert_eq!(wire_of(&d, w1), &Wire { id: w1, a: pref(r1, "p2"), b: pref(r2, "p1"), bends: vec![] });
}

#[test]
fn delete_components_with_their_wires_and_refusals() {
    let (mut d, r1, r2, w1) = two_resistors();
    let a = checked(&mut d, Edit::Delete { components: vec![r2], wires: vec![] });
    assert_eq!(a.patch.components.len(), 1);
    assert_eq!(a.patch.wires, vec![(w1, Some(Wire { id: w1, a: pref(r1, "p2"), b: pref(r2, "p1"), bends: vec![] }), None)]);
    refused_code(&mut d, Edit::Delete { components: vec![r2], wires: vec![] }, RefusalCode::UnknownComponent);
    refused_code(&mut d, Edit::Delete { components: vec![], wires: vec![w1] }, RefusalCode::UnknownWire);
    // Nothing selected: nothing committed.
    let rev = d.rev();
    assert!(edit(&mut d, Edit::Delete { components: vec![], wires: vec![] }).patch.is_empty());
    assert_eq!(d.rev(), rev);
}

// ------------------------------------------------------------------------------------------------ move, repair

#[test]
fn repair_straight_wire_becomes_a_z_keeping_its_orientation() {
    for (delta, expected) in [
        (pt(0, 4), vec![(3, 0), (6, 0), (6, 4), (9, 4)]),
        // x_mid = Q0.x + ⌊(Q1′.x − Q0.x)/2⌋ = 3 + ⌊7/2⌋ = 6.
        (pt(1, 4), vec![(3, 0), (6, 0), (6, 4), (10, 4)]),
        (pt(3, 0), vec![(3, 0), (12, 0)]),
    ] {
        let (mut d, _, r2, w1) = two_resistors();
        let a = checked(&mut d, Edit::Move { components: vec![r2], wires: vec![], delta });
        assert_eq!(path(&d, w1), pts(&expected), "delta {delta:?}");
        assert!(!a.topology_changed && !a.electrical_changed);
    }
    // The moving end is a: x_mid from the new Q0′.
    let (mut d, r1, _, w1) = two_resistors();
    checked(&mut d, Edit::Move { components: vec![r1], wires: vec![], delta: pt(0, -3) });
    assert_eq!(path(&d, w1), pts(&[(3, -3), (6, -3), (6, 0), (9, 0)]));
}

#[test]
fn repair_l_and_z_wires() {
    // m = 2: L from R1.p2 (3,0) via (6,0) to R3.p1 (6,5).
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    let w = wire(&mut d, pin(r1, "p2"), pin(r3, "p1"), Some(pts(&[(6, 0)])));
    checked(&mut d, Edit::Move { components: vec![r3], wires: vec![], delta: pt(2, 0) });
    assert_eq!(path(&d, w), pts(&[(3, 0), (8, 0), (8, 5)]));
    checked(&mut d, Edit::Move { components: vec![r1], wires: vec![], delta: pt(0, -2) });
    assert_eq!(path(&d, w), pts(&[(3, -2), (8, -2), (8, 5)]));
    // m = 3: Z from R1.p2 (3,0) to R4.p1 (9,4); moving R4 down slides the last horizontal segment.
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r4 = add(&mut d, Kind::Resistor, 12, 4, Rot::R0);
    let w = wire(&mut d, pin(r1, "p2"), pin(r4, "p1"), Some(pts(&[(5, 0), (5, 4)])));
    checked(&mut d, Edit::Move { components: vec![r4], wires: vec![], delta: pt(0, 2) });
    assert_eq!(path(&d, w), pts(&[(3, 0), (5, 0), (5, 6), (9, 6)]));
    // A selected wire without its ends: bends + delta, both ends repaired (segment drag).
    checked(&mut d, Edit::Move { components: vec![], wires: vec![w], delta: pt(1, 0) });
    assert_eq!(path(&d, w), pts(&[(3, 0), (6, 0), (6, 6), (9, 6)]));
    // Both ends in the moved set: rigid translation.
    let a = checked(&mut d, Edit::Move { components: vec![r1, r4], wires: vec![], delta: pt(-10, 20) });
    assert_eq!(path(&d, w), pts(&[(-7, 20), (-4, 20), (-4, 26), (-1, 26)]));
    assert_eq!(a.patch.wires.len(), 1);
}

#[test]
fn repair_falls_back_to_the_first_valid_autoroute_candidate() {
    let (mut d, _, r2, w1) = two_resistors();
    // A ground at (6,2) blocks the Z through x = 6; the L via (b.x, a.y) = (9, 0) is free.
    add(&mut d, Kind::Ground, 6, 2, Rot::R0);
    checked(&mut d, Edit::Move { components: vec![r2], wires: vec![], delta: pt(0, 6) });
    assert_eq!(path(&d, w1), pts(&[(3, 0), (9, 0), (9, 6)]));
}

#[test]
fn repair_refused_with_the_reason_of_the_first_candidate() {
    let (mut d, _, r2, _) = two_resistors();
    // R3 body (4,1)–(8,3), pins (3,2) and (9,2): the repaired Z through x = 6 crosses the body, and every autoroute
    // candidate from (3,0) to (9,4) meets the body or a pin. The first candidate, L via (9, 0), runs over R3.p2.
    add(&mut d, Kind::Resistor, 6, 2, Rot::R0);
    let r = ambiguous(&mut d, Edit::Move { components: vec![r2], wires: vec![], delta: pt(0, 4) }, Ambiguity::PinOnWire);
    assert_eq!(r.message_fr, "La broche R3.2 toucherait un fil sans y \u{ea}tre reli\u{e9}e");
    assert_eq!(r.at, Some(pt(9, 2)));
    refused_code(&mut d, Edit::Move { components: vec![CompId(77)], wires: vec![], delta: pt(0, 4) }, RefusalCode::UnknownComponent);
    refused_code(&mut d, Edit::Move { components: vec![r2], wires: vec![], delta: pt(20000, 0) }, RefusalCode::OutOfBounds);
    // A null move is not committed.
    let rev = d.rev();
    assert!(edit(&mut d, Edit::Move { components: vec![r2], wires: vec![], delta: pt(0, 0) }).patch.is_empty());
    assert_eq!(d.rev(), rev);
}

#[test]
fn junctions_follow_when_all_their_wires_lead_into_the_moved_set() {
    let mut d = tee();
    let ids = [CompId(1), CompId(2), CompId(4)];
    let a = checked(&mut d, Edit::Move { components: ids.to_vec(), wires: vec![], delta: pt(0, 10) });
    assert_eq!(comp(&d, CompId(5)).pos, pt(6, 10));
    assert!(a.patch.components.iter().any(|(id, _, _)| *id == CompId(5)));
    // Not all of them: the junction stays and the wires are repaired.
    let mut d = tee();
    checked(&mut d, Edit::Move { components: vec![CompId(1), CompId(2)], wires: vec![], delta: pt(0, -4) });
    assert_eq!(comp(&d, CompId(5)).pos, pt(6, 0));
    assert_eq!(path(&d, WireId(3)), pts(&[(3, -4), (4, -4), (4, 0), (6, 0)]));
}

// ------------------------------------------------------------------------------------------------ rotate

#[test]
fn rotate_single_component_about_its_origin() {
    let (mut d, r1, _, w1) = two_resistors();
    let a = checked(&mut d, Edit::Rotate { components: vec![r1], wires: vec![] });
    assert_eq!((comp(&d, r1).pos, comp(&d, r1).rot), (pt(0, 0), Rot::R90));
    assert_eq!(d.circuit().pin_pos(&pref(r1, "p2")), Some(pt(0, 3)));
    // W1's end a moved from (3,0) to (0,3): straight horizontal → Z, x_mid = 0 + ⌊9/2⌋ = 4.
    assert_eq!(path(&d, w1), pts(&[(0, 3), (4, 3), (4, 0), (9, 0)]));
    assert!(!a.electrical_changed);
    // A second quarter turn puts p2 at (−3,0) facing left: the repaired path is straight through the body, and every
    // candidate collapses onto that straight line. Refused with the first candidate's reason: it runs over R1.p1.
    let r = ambiguous(&mut d, Edit::Rotate { components: vec![r1], wires: vec![] }, Ambiguity::PinOnWire);
    assert_eq!((r.at, r.message_fr.as_str()), (Some(pt(3, 0)), "La broche R1.1 toucherait un fil sans y \u{ea}tre reli\u{e9}e"));
    // Without wires, four quarter turns come back to R0.
    let lone = add(&mut d, Kind::Capacitor, 40, 40, Rot::R0);
    for rot in [Rot::R90, Rot::R180, Rot::R270, Rot::R0] {
        checked(&mut d, Edit::Rotate { components: vec![lone], wires: vec![] });
        assert_eq!((comp(&d, lone).pos, comp(&d, lone).rot), (pt(40, 40), rot));
    }
}

#[test]
fn rotate_selection_about_the_snapped_centre() {
    let (mut d, r1, r2, w1) = two_resistors();
    // Box of bodies and pins: x −3..15, y −1..1 → c = (⌊6 + 0.5⌋, ⌊0 + 0.5⌋) = (6, 0).
    checked(&mut d, Edit::Rotate { components: vec![r1, r2], wires: vec![] });
    assert_eq!((comp(&d, r1).pos, comp(&d, r1).rot), (pt(6, -6), Rot::R90));
    assert_eq!((comp(&d, r2).pos, comp(&d, r2).rot), (pt(6, 6), Rot::R90));
    assert_eq!(path(&d, w1), pts(&[(6, -3), (6, 3)]));
    // Half-unit centre: R alone box x −3..3 plus a ground (body x 8..12): x −3..12 → cx = 4.5 → 5.
    let mut d = doc();
    let r = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let g = add(&mut d, Kind::Ground, 10, -1, Rot::R0);
    // y: resistor −1..1, ground −1..1 → cy = 0.
    checked(&mut d, Edit::Rotate { components: vec![r, g], wires: vec![] });
    assert_eq!(comp(&d, r).pos, pt(5, -5));
    assert_eq!(comp(&d, g).pos, pt(6, 5));
}

#[test]
fn rotating_a_lone_l_wire_flips_it() {
    let mut d = doc();
    let r1 = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let r3 = add(&mut d, Kind::Resistor, 6, 8, Rot::R90);
    let w = wire(&mut d, pin(r1, "p2"), pin(r3, "p1"), Some(pts(&[(6, 0)])));
    // Centre of the wire box (3,0)–(6,5): (5, 3); the rotated L starts vertically, both ends repaired back.
    checked(&mut d, Edit::Rotate { components: vec![], wires: vec![w] });
    assert_eq!(path(&d, w), pts(&[(3, 0), (3, 5), (6, 5)]));
}

// ------------------------------------------------------------------------------------------------ params, labels

#[test]
fn set_param_and_set_label() {
    let (mut d, r1, r2, _) = two_resistors();
    let a = checked(&mut d, Edit::SetParam { comp: r1, name: "resistance".into(), value: Value::Choice("22k".into()) });
    assert!(a.electrical_changed && !a.topology_changed);
    assert_eq!(comp(&d, r1).params["resistance"], Value::Num(22e3));
    let a = checked(&mut d, Edit::SetLabel { comp: r1, label: "Rpull".into() });
    assert!(!a.electrical_changed && !a.topology_changed);
    assert_eq!(comp(&d, r1).label, "Rpull");
    assert_eq!(d.circuit().netlist().net_of(&pref(r1, "p2")).unwrap().name, "Rpull.2");
    refused_code(&mut d, Edit::SetLabel { comp: r2, label: "Rpull".into() }, RefusalCode::LabelDuplicate);
    refused_code(&mut d, Edit::SetLabel { comp: r2, label: "_R".into() }, RefusalCode::LabelInvalid);
    refused_code(&mut d, Edit::SetLabel { comp: CompId(50), label: "R9".into() }, RefusalCode::UnknownComponent);
    refused_code(&mut d, Edit::SetParam { comp: CompId(50), name: "resistance".into(), value: Value::Num(1.0) }, RefusalCode::UnknownComponent);
    let g = add(&mut d, Kind::Ground, 30, 30, Rot::R0);
    refused_code(&mut d, Edit::SetLabel { comp: g, label: "G".into() }, RefusalCode::LabelInvalid);
    // Same label: empty patch.
    assert!(edit(&mut d, Edit::SetLabel { comp: r1, label: "Rpull".into() }).patch.is_empty());
}

// ------------------------------------------------------------------------------------------------ nets

#[test]
fn nets_and_names() {
    let mut d = doc();
    let v1 = add(&mut d, Kind::DcSupply, 0, 0, Rot::R0); // pos (0,−4), neg (0,4)
    let r1 = add(&mut d, Kind::Resistor, 10, -8, Rot::R0); // p1 (7,−8), p2 (13,−8)
    let g1 = add(&mut d, Kind::Ground, 0, 6, Rot::R0);
    let g2 = add(&mut d, Kind::Ground, 16, -4, Rot::R0);
    let adc = add(&mut d, Kind::AdcInput, 30, 0, Rot::R0);
    wire(&mut d, pin(v1, "pos"), pin(r1, "p1"), None);
    wire(&mut d, pin(v1, "neg"), pin(g1, "gnd"), None);
    wire(&mut d, pin(r1, "p2"), pin(g2, "gnd"), None);
    let nl = d.circuit().netlist();
    // Every pin in exactly one net; nets in key order.
    let mut all: Vec<_> = nl.nets.iter().flat_map(|n| n.pins.clone()).collect();
    let count = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), count);
    assert_eq!(count, 2 + 2 + 1 + 1 + 2);
    assert!(nl.nets.windows(2).all(|w| w[0].key < w[1].key));
    let gnd = nl.net_of(&pref(g1, "gnd")).unwrap();
    assert_eq!(gnd.name, "GND");
    assert!(gnd.ground);
    assert_eq!(gnd.pins, vec![pref(v1, "neg"), pref(r1, "p2"), pref(g1, "gnd"), pref(g2, "gnd")]);
    let top = nl.net_of(&pref(r1, "p1")).unwrap();
    assert_eq!((top.name.as_str(), &top.key), ("V1.+", &pref(v1, "pos")));
    // A local ground (ADC reference) is never joined implicitly.
    let refnet = nl.net_of(&pref(adc, "ref")).unwrap();
    assert_eq!((refnet.name.as_str(), refnet.pins.len(), refnet.ground), ("ADC1.REF", 1, false));
    // Ground symbols form one net even without a wire.
    let mut d = doc();
    let a = add(&mut d, Kind::Ground, 0, 0, Rot::R0);
    let b = add(&mut d, Kind::Ground, 10, 0, Rot::R0);
    let nl = d.circuit().netlist();
    assert_eq!(nl.nets.len(), 1);
    assert_eq!(nl.nets[0].pins, vec![pref(a, "gnd"), pref(b, "gnd")]);
}

#[test]
fn net_key_may_be_a_junction_while_the_name_comes_from_the_smallest_real_pin() {
    let mut d = tee(); // R1 1, R2 2, W1 3, R3 4, J 5, H 6, B 7
    let r4 = add(&mut d, Kind::Resistor, 0, -20, Rot::R0); // 8 "R4", p2 (3,−20)
    let r5 = add(&mut d, Kind::Resistor, 12, -20, Rot::R0); // 9 "R5", p1 (9,−20)
    let r6 = add(&mut d, Kind::Resistor, 6, 18, Rot::R90); // 10 "R6", p1 (6,15)
    // Move the three branches of J to the new resistors, clear of R1, R2 and R3.
    checked(&mut d, Edit::MoveWireEnd { wire: WireId(3), end: End::A, to: pin(r4, "p2"), bends: Some(pts(&[(3, -10), (6, -10)])) });
    let a = checked(&mut d, Edit::MoveWireEnd { wire: WireId(6), end: End::B, to: pin(r5, "p1"), bends: Some(pts(&[(7, 0), (7, -20)])) });
    assert!(a.topology_changed && a.electrical_changed);
    checked(&mut d, Edit::MoveWireEnd { wire: WireId(7), end: End::B, to: pin(r6, "p1"), bends: Some(pts(&[(4, 0), (4, 15)])) });
    checked(&mut d, Edit::Delete { components: vec![CompId(1), CompId(2), CompId(4)], wires: vec![] });
    assert_eq!(junctions(&d), vec![CompId(5)]);
    let nl = d.circuit().netlist();
    let net = nl.net_of(&pref(CompId(5), "j")).unwrap();
    // The junction (comp 5) is now the smallest pin of the net: the key; the name comes from R4.p2 (comp 8).
    assert_eq!(net.key, pref(CompId(5), "j"));
    assert_eq!(net.name, "R4.2");
    assert_eq!(net.pins, vec![pref(CompId(5), "j"), pref(r4, "p2"), pref(r5, "p1"), pref(r6, "p1")]);
    assert_eq!(net.wires, vec![WireId(3), WireId(6), WireId(7)]);
    // A net of junctions only would be named "J<id>"; the other pins of R4, R5, R6 are nets of one pin.
    assert_eq!(nl.net_of(&pref(r4, "p1")).unwrap().name, "R4.1");
    assert_eq!(nl.net_of(&pref(r6, "p2")).unwrap().pins.len(), 1);
}

#[test]
fn rotation_keeps_wires_attached_in_every_quarter() {
    // R1 at the origin, its p2 wired to a ground pin at (20,20) (lead from below, pointing up).
    let mut d = doc();
    let r = add(&mut d, Kind::Resistor, 0, 0, Rot::R0);
    let g = add(&mut d, Kind::Ground, 20, 20, Rot::R0);
    let w = wire(&mut d, pin(r, "p2"), pin(g, "gnd"), None);
    assert_eq!(path(&d, w), pts(&[(3, 0), (20, 0), (20, 20)]));
    let expected = [
        // R90: p2 (0,3); m = 2, first segment horizontal: Q1.y ← 3.
        (Rot::R90, vec![(0, 3), (20, 3), (20, 20)]),
        // R180: p2 (−3,0); the repair runs through the body, the L via (b.x, a.y) too: L via (a.x, b.y).
        (Rot::R180, vec![(-3, 0), (-3, 20), (20, 20)]),
        // R270: p2 (0,−3); the repair crosses the body, the first L is free.
        (Rot::R270, vec![(0, -3), (20, -3), (20, 20)]),
        // R0: back to the original path by the plain repair.
        (Rot::R0, vec![(3, 0), (20, 0), (20, 20)]),
    ];
    for (rot, want) in expected {
        let nets = d.circuit().netlist();
        let rev = d.rev();
        let a = checked(&mut d, Edit::Rotate { components: vec![r], wires: vec![] });
        assert_eq!((a.rev, comp(&d, r).rot), (rev + 1, rot));
        assert_eq!(path(&d, w), pts(&want), "{rot:?}");
        assert_eq!(d.circuit().netlist(), nets, "a rotation never changes the nets");
        assert!(!a.topology_changed && !a.electrical_changed);
    }
    // An ECU with a wire from A0 to the right: a quarter turn is repaired, a half turn would need a detour around the
    // body that no candidate offers. The first candidate, L via (27, 4), runs over the 5V pin.
    let mut d = doc();
    let u = add(&mut d, Kind::EcuStm32, 0, 0, Rot::R0);
    let r = add(&mut d, Kind::Resistor, 30, -4, Rot::R0);
    let w = wire(&mut d, pin(u, "a0"), pin(r, "p1"), None);
    checked(&mut d, Edit::Rotate { components: vec![u], wires: vec![] });
    // a0 (8,−4) → (4,8); straight → Z with x_mid = 4 + ⌊23/2⌋ = 15.
    assert_eq!(path(&d, w), pts(&[(4, 8), (15, 8), (15, -4), (27, -4)]));
    let refusal = ambiguous(&mut d, Edit::Rotate { components: vec![u], wires: vec![] }, Ambiguity::PinOnWire);
    assert_eq!((refusal.at, refusal.message_fr.as_str()), (Some(pt(8, 4)), "La broche U1.5V toucherait un fil sans y \u{ea}tre reli\u{e9}e"));
}

//! Circuit builder of the Physical Lab engine tests, on the document API only (n7rt-circuit). Included by
//! `phys_kit.rs` and by the unit tests of `src/phys` (`#[path]`), so that both build the same circuits.
//!
//! [`Bench`] routes without any possible ambiguity: components in a row (R0, 60 units apart), every pin led up
//! to a horizontal corridor of its own net above everything, the other pins of a net attached to that corridor
//! by a junction. [`ref_pot_rc`] and [`tp1_capteur_can`] rebuild the netlists of contract §9.1 and §9.2 with the
//! library defaults, independently of the embedded demonstration files.

#![allow(dead_code)]

use n7rt_circuit::{kind_def, Circuit, CompId, Dir, Document, Edit, Endpoint, Interact, Kind, PinRef, Pt, Rot, Value, ValueMap, WireId};

pub const SPACING: i32 = 60;

pub struct Bench {
    doc: Document,
    next_x: i32,
    nets: Vec<(String, Vec<PinRef>)>,
}

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Bench {
    pub fn new() -> Bench {
        Bench { doc: Document::new(Circuit::new("banc")), next_x: 0, nets: Vec::new() }
    }

    pub fn add(&mut self, kind: Kind, params: &[(&str, Value)]) -> CompId {
        let params: ValueMap = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let edit = Edit::AddComponent { kind, pos: Pt { x: self.next_x, y: 0 }, rot: Rot::R0, params, label: None };
        self.next_x += SPACING;
        let applied = self.doc.edit(self.doc.rev(), &edit).unwrap_or_else(|r| panic!("{kind:?} placed: {}", r.message_fr));
        new_comp(&applied.patch.components)
    }

    /// Adds pins to the named net.
    pub fn join(&mut self, net: &str, pins: &[(CompId, &str)]) {
        let refs = pins.iter().map(|(c, p)| PinRef { comp: *c, pin: p.to_string() });
        match self.nets.iter_mut().find(|(n, _)| n == net) {
            Some((_, v)) => v.extend(refs),
            None => self.nets.push((net.to_string(), refs.collect())),
        }
    }

    pub fn interact(&mut self, i: Interact) {
        self.doc.interact(&i).unwrap_or_else(|r| panic!("interaction: {}", r.message_fr));
    }

    pub fn build(mut self) -> Circuit {
        let nets = std::mem::take(&mut self.nets);
        for (k, (_, pins)) in nets.iter().enumerate() {
            if pins.len() >= 2 {
                self.route(pins, -40 - 2 * k as i32);
            }
        }
        let c = self.doc.circuit().clone();
        assert!(c.check().is_empty(), "bench circuit valid: {:?}", c.check());
        c
    }

    fn comp(&self, id: CompId) -> &n7rt_circuit::Component {
        &self.doc.circuit().components()[&id]
    }

    /// Path from a pin up to its corridor: straight for an upward lead, a stub then up for a side lead (lower
    /// pins of a side get longer stubs), around the right of the body for a downward lead.
    fn path_up(&self, p: &PinRef, yc: i32) -> Vec<Pt> {
        let c = self.comp(p.comp);
        let def = kind_def(c.kind);
        let d = def.pins.iter().find(|d| d.id == p.pin).expect("library pin");
        let at = Pt { x: c.pos.x + d.pos.x, y: c.pos.y + d.pos.y };
        let stub = |dir: Dir, y: i32| {
            let mut ys: Vec<i32> = def.pins.iter().filter(|q| q.dir == dir).map(|q| q.pos.y).collect();
            ys.sort();
            2 + 2 * ys.iter().position(|v| *v == y).expect("same side") as i32
        };
        match d.dir {
            Dir::U => vec![at, Pt { x: at.x, y: yc }],
            Dir::L => {
                let s = stub(Dir::L, d.pos.y);
                vec![at, Pt { x: at.x - s, y: at.y }, Pt { x: at.x - s, y: yc }]
            }
            Dir::R => {
                let s = stub(Dir::R, d.pos.y);
                vec![at, Pt { x: at.x + s, y: at.y }, Pt { x: at.x + s, y: yc }]
            }
            Dir::D => {
                let rights = def.pins.iter().filter(|q| q.dir == Dir::R).count() as i32;
                let xa = c.pos.x + def.body.x1 + 1 + 2 * rights + 2;
                vec![at, Pt { x: at.x, y: at.y + 2 }, Pt { x: xa, y: at.y + 2 }, Pt { x: xa, y: yc }]
            }
        }
    }

    fn route(&mut self, pins: &[PinRef], yc: i32) {
        let mut legs: Vec<(PinRef, Vec<Pt>)> = pins.iter().map(|p| (p.clone(), self.path_up(p, yc))).collect();
        legs.sort_by_key(|(_, path)| path.last().expect("path").x);
        let (first, last) = (&legs[0], &legs[legs.len() - 1]);
        let mut path = first.1.clone();
        path.extend(last.1.iter().rev());
        let edit = Edit::AddWire {
            a: Endpoint::Pin { comp: first.0.comp, pin: first.0.pin.clone() },
            b: Endpoint::Pin { comp: last.0.comp, pin: last.0.pin.clone() },
            bends: Some(path[1..path.len() - 1].to_vec()),
        };
        self.doc.edit(self.doc.rev(), &edit).unwrap_or_else(|r| panic!("corridor wire at y = {yc}: {}", r.message_fr));
        for (pin, path) in &legs[1..legs.len() - 1] {
            let at = *path.last().expect("path");
            let wire = self.wire_through(at);
            let edit = Edit::AddWire {
                a: Endpoint::Pin { comp: pin.comp, pin: pin.pin.clone() },
                b: Endpoint::OnWire { wire, at },
                bends: Some(path[1..path.len() - 1].to_vec()),
            };
            self.doc.edit(self.doc.rev(), &edit).unwrap_or_else(|r| panic!("branch to {at:?}: {}", r.message_fr));
        }
    }

    fn wire_through(&self, at: Pt) -> WireId {
        let c = self.doc.circuit();
        for id in c.wires().keys() {
            let path = c.wire_path(*id).expect("wire path");
            let on = path.windows(2).any(|s| {
                let (a, b) = (s[0], s[1]);
                (a.x == b.x && at.x == a.x && at.y >= a.y.min(b.y) && at.y <= a.y.max(b.y))
                    || (a.y == b.y && at.y == a.y && at.x >= a.x.min(b.x) && at.x <= a.x.max(b.x))
            });
            if on {
                return *id;
            }
        }
        panic!("no wire through {at:?}");
    }
}

fn new_comp(entries: &[(CompId, Option<n7rt_circuit::Component>, Option<n7rt_circuit::Component>)]) -> CompId {
    entries.iter().find(|(_, b, a)| b.is_none() && a.is_some()).map(|(id, _, _)| *id).expect("a new component")
}

/// Contract §9.1: V1 → S1 (high side, closed) → RV1.b, RV1.a at ground, RV1 at 0.3; W → ADC1 and R1; R1 → X;
/// C1 from X to ground; X → ADC2. ADCs: vref 5 V, 12 bits, ideal inputs. Designators V1, S1, RV1, R1, C1, ADC1,
/// ADC2 (placement order).
pub fn ref_pot_rc() -> Circuit {
    let mut b = Bench::new();
    let v1 = b.add(Kind::DcSupply, &[]);
    let g = b.add(Kind::Ground, &[]);
    let s1 = b.add(Kind::Switch, &[]);
    let rv1 = b.add(Kind::Potentiometer, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    let c1 = b.add(Kind::Capacitor, &[]);
    let adc1 = b.add(Kind::AdcInput, &[("vref", Value::Num(5.0))]);
    let adc2 = b.add(Kind::AdcInput, &[("vref", Value::Num(5.0))]);
    b.join("v", &[(v1, "pos"), (s1, "p1")]);
    b.join("b", &[(s1, "p2"), (rv1, "b")]);
    b.join("gnd", &[(g, "gnd"), (v1, "neg"), (rv1, "a"), (c1, "p2"), (adc1, "ref"), (adc2, "ref")]);
    b.join("w", &[(rv1, "w"), (adc1, "in"), (r1, "p1")]);
    b.join("x", &[(r1, "p2"), (c1, "p1"), (adc2, "in")]);
    b.interact(Interact::SetSwitch { comp: s1, closed: true });
    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.build()
}

/// Contract §9.2: the CAN wiring of the real bench plus two analog inputs. Designators U1 (ECU), RV1, R1, C1, U2
/// (MCP2551), RT1, BUS1 (placement order).
pub fn tp1_capteur_can() -> Circuit {
    let mut b = Bench::new();
    let u1 = b.add(Kind::EcuStm32, &[]);
    let g = b.add(Kind::Ground, &[]);
    let rv1 = b.add(Kind::Potentiometer, &[]);
    let r1 = b.add(Kind::Resistor, &[]);
    let c1 = b.add(Kind::Capacitor, &[]);
    let u2 = b.add(Kind::CanTransceiver, &[]);
    let rt1 = b.add(Kind::Termination, &[]);
    let bus1 = b.add(Kind::CanBus, &[]);
    b.join("3v3", &[(u1, "v3v3"), (rv1, "b")]);
    b.join("gnd", &[(g, "gnd"), (u1, "gnd"), (rv1, "a"), (c1, "p2"), (u2, "vss"), (u2, "rs"), (bus1, "gnd")]);
    b.join("a0", &[(rv1, "w"), (u1, "a0"), (r1, "p1")]);
    b.join("a1", &[(r1, "p2"), (c1, "p1"), (u1, "a1")]);
    b.join("5v", &[(u1, "v5"), (u2, "vdd")]);
    b.join("tx", &[(u1, "can_tx"), (u2, "txd")]);
    b.join("rx", &[(u2, "rxd"), (u1, "can_rx")]);
    b.join("canh", &[(u2, "canh"), (rt1, "p1"), (bus1, "canh")]);
    b.join("canl", &[(u2, "canl"), (rt1, "p2"), (bus1, "canl")]);
    b.interact(Interact::SetPot { comp: rv1, position: 0.3 });
    b.build()
}

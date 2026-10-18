//! Helpers shared by the integration tests: a document driver, a hand-written PRNG and a random edit generator.
#![allow(dead_code)]

use n7rt_circuit::{
    geom, kind_def, library, persist, Applied, Circuit, CompId, Document, Edit, End, Endpoint, Interact, Kind, Patch,
    PinRef, Pt, Refusal, RefusalCode, Rot, Value, ValueMap, ValueType, WireId,
};

pub fn pt(x: i32, y: i32) -> Pt {
    Pt { x, y }
}

pub fn pin(comp: CompId, pin: &str) -> Endpoint {
    Endpoint::Pin { comp, pin: pin.to_string() }
}

pub fn on_wire(wire: WireId, x: i32, y: i32) -> Endpoint {
    Endpoint::OnWire { wire, at: pt(x, y) }
}

pub fn pref(comp: CompId, pin: &str) -> PinRef {
    PinRef { comp, pin: pin.to_string() }
}

pub fn doc() -> Document {
    Document::new(Circuit::new("essai"))
}

pub fn try_edit(d: &mut Document, e: Edit) -> Result<Applied, Refusal> {
    let rev = d.rev();
    d.edit(rev, &e)
}

pub fn edit(d: &mut Document, e: Edit) -> Applied {
    let rev = d.rev();
    match d.edit(rev, &e) {
        Ok(a) => a,
        Err(r) => panic!("{e:?} refused: {} ({:?} {:?})", r.message_fr, r.code, r.ambiguity),
    }
}

/// The refusal of an edit that must be refused, after checking that nothing changed.
pub fn refused(d: &mut Document, e: Edit) -> Refusal {
    let before = d.circuit().clone();
    let rev = d.rev();
    let r = match d.edit(rev, &e) {
        Ok(a) => panic!("{e:?} accepted: {:?}", a.patch),
        Err(r) => r,
    };
    assert_eq!(d.circuit(), &before, "a refused edit changed the circuit");
    assert_eq!(d.rev(), rev);
    r
}

pub fn refused_code(d: &mut Document, e: Edit, code: RefusalCode) -> Refusal {
    let r = refused(d, e);
    assert_eq!(r.code, code, "{}", r.message_fr);
    r
}

pub fn add_with(d: &mut Document, kind: Kind, x: i32, y: i32, rot: Rot, params: &[(&str, Value)]) -> CompId {
    let params: ValueMap = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let a = edit(d, Edit::AddComponent { kind, pos: pt(x, y), rot, params, label: None });
    a.patch.components.iter().find(|(_, b, _)| b.is_none()).map(|(id, _, _)| *id).expect("a new component")
}

pub fn add(d: &mut Document, kind: Kind, x: i32, y: i32, rot: Rot) -> CompId {
    add_with(d, kind, x, y, rot, &[])
}

/// New wire id of an AddWire (the last id created).
pub fn wire(d: &mut Document, a: Endpoint, b: Endpoint, bends: Option<Vec<Pt>>) -> WireId {
    let ap = edit(d, Edit::AddWire { a, b, bends });
    ap.patch.wires.iter().filter(|(_, b, _)| b.is_none()).map(|(id, _, _)| *id).max().expect("a new wire")
}

pub fn path(d: &Document, w: WireId) -> Vec<Pt> {
    d.circuit().wire_path(w).expect("wire exists")
}

pub fn pts(list: &[(i32, i32)]) -> Vec<Pt> {
    list.iter().map(|&(x, y)| pt(x, y)).collect()
}

pub fn assert_valid(d: &Document) {
    let v = d.circuit().check();
    assert!(v.is_empty(), "invariants broken: {v:#?}");
}

/// xorshift64* (Vigna 2016): deterministic, no dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len() as u64) as usize])
        }
    }

    pub fn weighted<T: Copy>(&mut self, items: &[(T, u64)]) -> T {
        let total: u64 = items.iter().map(|(_, w)| w).sum();
        let mut k = self.below(total);
        for (item, w) in items {
            if k < *w {
                return *item;
            }
            k -= w;
        }
        items[items.len() - 1].0
    }

    /// Up to `max` distinct items.
    pub fn subset<T: Copy + PartialEq>(&mut self, items: &[T], max: usize) -> Vec<T> {
        let n = self.below(max as u64 + 1) as usize;
        let mut out = Vec::new();
        for _ in 0..n {
            if let Some(x) = self.pick(items) {
                if !out.contains(x) {
                    out.push(*x);
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------- patch and text checks

/// The patch is exactly the difference of the component and wire tables, ascending ids.
pub fn assert_patch_exact(before: &Circuit, after: &Circuit, patch: &Patch) {
    let mut ids: Vec<u32> = before.components().keys().chain(after.components().keys()).map(|c| c.0).collect();
    ids.sort();
    ids.dedup();
    let expected: Vec<_> = ids
        .into_iter()
        .map(CompId)
        .filter_map(|id| {
            let (b, a) = (before.components().get(&id).cloned(), after.components().get(&id).cloned());
            (b != a).then_some((id, b, a))
        })
        .collect();
    assert_eq!(patch.components, expected, "component part of the patch");
    let mut ids: Vec<u32> = before.wires().keys().chain(after.wires().keys()).map(|w| w.0).collect();
    ids.sort();
    ids.dedup();
    let expected: Vec<_> = ids
        .into_iter()
        .map(WireId)
        .filter_map(|id| {
            let (b, a) = (before.wires().get(&id).cloned(), after.wires().get(&id).cloned());
            (b != a).then_some((id, b, a))
        })
        .collect();
    assert_eq!(patch.wires, expected, "wire part of the patch");
}

/// Canonical text without `next_id` (never restored by undo) and without the interaction states (outside undo).
pub fn masked_text(c: &Circuit) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&persist::save(c)).expect("canonical text is JSON");
    let root = v.as_object_mut().expect("object");
    root.remove("next_id");
    for comp in root.get_mut("components").and_then(|c| c.as_array_mut()).expect("components") {
        comp.as_object_mut().expect("component").remove("state");
    }
    serde_json::to_string_pretty(&v).expect("serializable")
}

// ------------------------------------------------------------------------------------------- random edits

const KINDS: [(Kind, u64); 12] = [
    (Kind::Resistor, 6),
    (Kind::Capacitor, 3),
    (Kind::Ground, 4),
    (Kind::Switch, 2),
    (Kind::PushButton, 2),
    (Kind::Potentiometer, 2),
    (Kind::DcSupply, 2),
    (Kind::AdcInput, 2),
    (Kind::Termination, 1),
    (Kind::EcuStm32, 1),
    (Kind::CanTransceiver, 1),
    (Kind::CanBus, 1),
];

const LABELS: [&str; 8] = ["R1", "R2", "X1", "Rload", "U1", "1A", "bad label", "Sw_main"];

const E12: [&str; 12] = ["1", "1.2", "1.5", "1.8", "2.2", "2.7", "3.3", "3.9", "4.7", "5.6", "6.8", "8.2"];
const PREFIXES: [(&str, i32); 8] = [("p", -12), ("n", -9), ("\u{b5}", -6), ("u", -6), ("m", -3), ("", 0), ("k", 3), ("M", 6)];

/// A parameter value: mostly valid (text with prefix and unit, RKM, typed), sometimes refused.
fn random_param_value(rng: &mut Rng, pd: &n7rt_circuit::ParamDef) -> Value {
    match pd.vtype {
        ValueType::Number | ValueType::NumberOrInf => match rng.below(12) {
            0 => Value::Choice("abc".to_string()),
            1 => Value::Choice(format!("5 {}", if pd.unit == "V" { "A" } else { "V" })),
            2 => Value::Num(pd.max * 10.0 + 1.0),
            3 if pd.vtype == ValueType::NumberOrInf => Value::Choice("inf".to_string()),
            4 => Value::Num(pd.min),
            5 => Value::Int(pd.max as i64),
            _ => {
                for _ in 0..8 {
                    let m = *rng.pick(&E12).expect("E12");
                    let (p, e) = *rng.pick(&PREFIXES).expect("prefixes");
                    let x: f64 = format!("{m}e{e}").parse().expect("literal");
                    if (pd.min..=pd.max).contains(&x) {
                        let text = match rng.below(3) {
                            0 => format!("{m}{p}"),
                            1 => format!("{} {p}{}", m.replace('.', ","), pd.unit),
                            _ if !p.is_empty() && m.contains('.') => m.replace('.', p),
                            _ => format!("{m}{p}{}", pd.unit),
                        };
                        return Value::Choice(text);
                    }
                }
                Value::Num(pd.min)
            }
        },
        ValueType::Int if pd.name.ends_with("sample_rate") => {
            Value::Int(*rng.pick(&[1, 2, 4, 5, 8, 10, 3, 100, 125, 250, 300, 1000]).expect("rates"))
        }
        ValueType::Int => {
            let (lo, hi) = (pd.min as i64, pd.max as i64);
            Value::Int(*rng.pick(&[lo, hi, lo - 1, hi + 1, (lo + hi) / 2]).expect("ints"))
        }
        ValueType::Bool => Value::Bool(rng.chance(50)),
        ValueType::Choice => {
            if rng.chance(15) {
                Value::Choice("zzz".to_string())
            } else {
                Value::Choice(rng.pick(pd.choices).expect("choices").to_string())
            }
        }
    }
}

fn comp_ids(c: &Circuit) -> Vec<CompId> {
    c.components().keys().copied().collect()
}

fn wire_ids(c: &Circuit) -> Vec<WireId> {
    c.wires().keys().copied().collect()
}

/// A random grid point of a wire path (an end, a bend or an interior point), sometimes off the path.
fn random_point_on(rng: &mut Rng, c: &Circuit, w: WireId) -> Pt {
    let path = c.wire_path(w).expect("wire exists");
    let k = rng.below(path.len() as u64 - 1) as usize;
    let (a, b) = (path[k], path[k + 1]);
    let t = rng.range(0, (b.x - a.x).abs().max((b.y - a.y).abs()));
    let p = pt(a.x + t * (b.x - a.x).signum(), a.y + t * (b.y - a.y).signum());
    if rng.chance(4) {
        pt(p.x + 1, p.y + 1)
    } else {
        p
    }
}

/// A pin of a random component, or a point on a random wire. None in an empty circuit.
pub fn random_endpoint(rng: &mut Rng, c: &Circuit) -> Option<Endpoint> {
    let wires = wire_ids(c);
    if !wires.is_empty() && rng.chance(30) {
        let w = *rng.pick(&wires)?;
        return Some(Endpoint::OnWire { wire: w, at: random_point_on(rng, c, w) });
    }
    let comp = *rng.pick(&comp_ids(c))?;
    let pins = kind_def(c.components()[&comp].kind).pins;
    let pin = if rng.chance(3) { "zz".to_string() } else { rng.pick(pins)?.id.to_string() };
    Some(Endpoint::Pin { comp, pin })
}

fn endpoint_pos(c: &Circuit, e: &Endpoint) -> Option<Pt> {
    match e {
        Endpoint::Pin { comp, pin } => c.pin_pos(&PinRef { comp: *comp, pin: pin.clone() }),
        Endpoint::OnWire { at, .. } => Some(*at),
    }
}

/// Explicit bends between two points: an L, a Z, or (rarely) a diagonal that is refused.
fn random_bends(rng: &mut Rng, a: Pt, b: Pt) -> Vec<Pt> {
    match rng.below(5) {
        0 => vec![pt(b.x, a.y)],
        1 => vec![pt(a.x, b.y)],
        2 => {
            let xm = rng.range(a.x.min(b.x) - 3, a.x.max(b.x) + 3);
            vec![pt(xm, a.y), pt(xm, b.y)]
        }
        3 => {
            let ym = rng.range(a.y.min(b.y) - 3, a.y.max(b.y) + 3);
            vec![pt(a.x, ym), pt(b.x, ym)]
        }
        _ => vec![pt(a.x + 1, a.y + 1)],
    }
}

/// New bends for an existing wire: an autoroute candidate, an L/Z, or a perpendicular drag of an inner segment
/// (its neighbours stretched to stay orthogonal, as the canvas does).
fn reshaped(rng: &mut Rng, c: &Circuit, w: WireId) -> Vec<Pt> {
    let wire = &c.wires()[&w];
    let path = c.wire_path(w).expect("wire exists");
    let (a, b) = (path[0], path[path.len() - 1]);
    match rng.below(3) {
        0 => {
            let (da, db) = (c.pin_dir(&wire.a).expect("pin"), c.pin_dir(&wire.b).expect("pin"));
            let cands = geom::autoroute(a, da, b, db);
            rng.pick(&cands).cloned().unwrap_or_default()
        }
        1 if path.len() >= 4 => {
            // Segment (k, k+1) between two bends moves by d perpendicular to itself.
            let k = 1 + rng.below(path.len() as u64 - 3) as usize;
            let d = *rng.pick(&[-3, -2, -1, 1, 2, 3]).expect("offsets");
            let mut p = path.clone();
            if p[k].y == p[k + 1].y {
                p[k].y += d;
                p[k + 1].y += d;
            } else {
                p[k].x += d;
                p[k + 1].x += d;
            }
            p[1..p.len() - 1].to_vec()
        }
        _ => random_bends(rng, a, b),
    }
}

pub fn random_edit(rng: &mut Rng, c: &Circuit) -> Edit {
    let comps = comp_ids(c);
    let wires = wire_ids(c);
    let op = if comps.is_empty() {
        0
    } else {
        rng.weighted(&[(0, 20), (1, 22), (2, 8), (3, 14), (4, 8), (5, 8), (6, 4), (7, 8), (8, 8)])
    };
    match op {
        1 => {
            let (Some(a), Some(b)) = (random_endpoint(rng, c), random_endpoint(rng, c)) else { return random_add(rng) };
            let bends = match (rng.chance(40), endpoint_pos(c, &a), endpoint_pos(c, &b)) {
                (true, Some(pa), Some(pb)) => Some(random_bends(rng, pa, pb)),
                _ => None,
            };
            Edit::AddWire { a, b, bends }
        }
        2 => {
            let mut components = rng.subset(&comps, 2);
            let wires = rng.subset(&wires, 1);
            if components.is_empty() && wires.is_empty() {
                components.push(*rng.pick(&comps).expect("components"));
            }
            Edit::Delete { components, wires }
        }
        3 => Edit::Move {
            components: rng.subset(&comps, 3),
            wires: rng.subset(&wires, 1),
            delta: pt(rng.range(-5, 5), rng.range(-5, 5)),
        },
        4 if !wires.is_empty() && rng.chance(15) => Edit::Rotate { components: vec![], wires: rng.subset(&wires, 1) },
        4 => Edit::Rotate { components: rng.subset(&comps, 2), wires: vec![] },
        5 => {
            let comp = *rng.pick(&comps).expect("components");
            let params = kind_def(c.components()[&comp].kind).params;
            match rng.pick(params) {
                Some(pd) => Edit::SetParam { comp, name: pd.name.to_string(), value: random_param_value(rng, pd) },
                None => Edit::SetParam { comp, name: "foo".to_string(), value: Value::Num(1.0) },
            }
        }
        6 => Edit::SetLabel {
            comp: *rng.pick(&comps).expect("components"),
            label: if rng.chance(50) { format!("Q{}", rng.below(5)) } else { rng.pick(&LABELS).expect("labels").to_string() },
        },
        7 if !wires.is_empty() => {
            let wire = *rng.pick(&wires).expect("wires");
            let to = random_endpoint(rng, c).expect("non-empty circuit");
            let end = if rng.chance(50) { End::A } else { End::B };
            Edit::MoveWireEnd { wire, end, to, bends: None }
        }
        8 if !wires.is_empty() => {
            let wire = *rng.pick(&wires).expect("wires");
            Edit::SetBends { wire, bends: reshaped(rng, c, wire) }
        }
        _ => random_add(rng),
    }
}

fn random_add(rng: &mut Rng) -> Edit {
    let kind = rng.weighted(&KINDS);
    let rot = *rng.pick(&[Rot::R0, Rot::R90, Rot::R180, Rot::R270]).expect("rotations");
    let mut params = ValueMap::new();
    if rng.chance(25) {
        if let Some(pd) = rng.pick(kind_def(kind).params) {
            params.insert(pd.name.to_string(), random_param_value(rng, pd));
        }
    }
    let label = rng.chance(10).then(|| rng.pick(&LABELS).expect("labels").to_string());
    Edit::AddComponent { kind, pos: pt(rng.range(-40, 40), rng.range(-30, 30)), rot, params, label }
}

pub fn random_interact(rng: &mut Rng, c: &Circuit) -> Option<Interact> {
    let comp = *rng.pick(&comp_ids(c))?;
    // Mostly the interaction of the component's kind, sometimes a wrong one (not_interactive).
    let fitting = match c.components()[&comp].kind {
        Kind::Potentiometer => Some(0),
        Kind::PushButton => Some(1),
        Kind::Switch => Some(2),
        Kind::DcSupply | Kind::EcuStm32 => Some(3),
        _ => None,
    };
    let which = match fitting {
        Some(k) if rng.chance(75) => k,
        _ => rng.below(4),
    };
    Some(match which {
        0 => {
            let position = match rng.below(6) {
                0 => 1.5,
                1 => 0.12345,
                _ => rng.below(1001) as f64 / 1000.0,
            };
            Interact::SetPot { comp, position }
        }
        1 => Interact::SetButton { comp, pressed: rng.chance(50) },
        2 => Interact::SetSwitch { comp, closed: rng.chance(50) },
        _ => Interact::SetPower { comp, on: rng.chance(50) },
    })
}

/// A document built by `steps` random edits (refusals ignored) and interactions.
pub fn random_document(rng: &mut Rng, name: &str, steps: usize) -> Document {
    let mut d = Document::new(Circuit::new(name));
    for _ in 0..steps {
        if rng.chance(80) || d.circuit().components().is_empty() {
            let e = random_edit(rng, d.circuit());
            let rev = d.rev();
            let _ = d.edit(rev, &e);
        } else if let Some(i) = random_interact(rng, d.circuit()) {
            let _ = d.interact(&i);
        }
    }
    d
}

/// Every placeable kind of the library.
pub fn placeable_kinds() -> Vec<Kind> {
    library().iter().filter(|k| k.placeable).map(|k| k.kind).collect()
}

//! n7rt-circuit — schematic document of the N7RT Physical Lab.
//!
//! Contract: docs/physical-lab-v1.md §1–§4, §7, §9. This crate is the single source of truth for what the student
//! built: component library (data), components and their pins, wires pin→pin, junctions, nets, edit transactions
//! with exact undo/redo patches, interaction states, canonical persistence and the embedded demonstrations.
//! No numerics (n7rt-solver), no engine and no electrical semantics (n7rt-lab::phys), no rendering: the UI keeps a
//! read-only mirror updated from [`Patch`]es and never decides anything itself.
//!
//! Public items below are frozen for the parallel implementation lots; private modules are free.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

mod catalog;
mod check;
mod demos;
mod edit;
mod model;
mod netlist;
mod store;
mod units;
mod validate;
mod values;

pub use catalog::SAMPLE_RATES_HZ;

// ------------------------------------------------------------------------------------------------- identifiers

/// Component id. Ids of components and wires come from one counter, [`Circuit::next_id`], which only grows and is
/// never restored by undo: an id is never reused in a circuit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WireId(pub u32);

/// A pin of a component; `pin` is the library pin id ("p1", "w", "vdd"). Ordered by (comp, pin). JSON
/// `{"comp": 3, "pin": "p1"}`. Stable while the component exists: instruments and missions address pins.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub comp: CompId,
    pub pin: String,
}

/// Identity of a net: its smallest [`PinRef`]. It changes when a smaller pin joins or leaves the net, so it is for
/// display and diffing only; durable references use [`PinRef`].
pub type NetKey = PinRef;

// ---------------------------------------------------------------------------------------------------- geometry

/// Grid point, in grid units (1 unit = 10 CSS px at zoom 1), x to the right, y down. JSON `[x, y]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(from = "[i32; 2]", into = "[i32; 2]")]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

impl From<[i32; 2]> for Pt {
    fn from([x, y]: [i32; 2]) -> Self {
        Pt { x, y }
    }
}

impl From<Pt> for [i32; 2] {
    fn from(p: Pt) -> Self {
        [p.x, p.y]
    }
}

/// Every stored coordinate satisfies |x| ≤ COORD_LIMIT and |y| ≤ COORD_LIMIT (refusal `out_of_bounds`).
pub const COORD_LIMIT: i32 = 10_000;

/// Clockwise rotation about the component origin (y down): R90 maps (x, y) to (−y, x), R180 to (−x, −y), R270 to
/// (y, −x). No mirror in V1. JSON 0 | 90 | 180 | 270.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum Rot {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl TryFrom<u16> for Rot {
    type Error = String;
    fn try_from(v: u16) -> Result<Self, String> {
        match v {
            0 => Ok(Rot::R0),
            90 => Ok(Rot::R90),
            180 => Ok(Rot::R180),
            270 => Ok(Rot::R270),
            _ => Err(format!("rotation {v} : attendu 0, 90, 180 ou 270")),
        }
    }
}

impl From<Rot> for u16 {
    fn from(r: Rot) -> u16 {
        match r {
            Rot::R0 => 0,
            Rot::R90 => 90,
            Rot::R180 => 180,
            Rot::R270 => 270,
        }
    }
}

/// Outward direction of a pin lead, from the body towards the connection point; rotates with the component.
/// JSON "l" | "r" | "u" | "d".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    L,
    R,
    U,
    D,
}

/// Axis-aligned rectangle in grid units, bounds inclusive (x0 ≤ x1, y0 ≤ y1). JSON `{"x0":…,"y0":…,"x1":…,"y1":…}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// Geometry rules shared by edits, validation and load (contract §3.4). Pure functions.
pub mod geom {
    use super::{Dir, Pt, Rot};

    /// `p` rotated by `r` about the origin.
    pub fn rotate(p: Pt, r: Rot) -> Pt {
        match r {
            Rot::R0 => p,
            Rot::R90 => Pt { x: p.y.saturating_neg(), y: p.x },
            Rot::R180 => Pt { x: p.x.saturating_neg(), y: p.y.saturating_neg() },
            Rot::R270 => Pt { x: p.y, y: p.x.saturating_neg() },
        }
    }

    pub fn rotate_dir(d: Dir, r: Rot) -> Dir {
        // Clockwise quarter turn with y down: right → down → left → up.
        let cw = |d: Dir| match d {
            Dir::R => Dir::D,
            Dir::D => Dir::L,
            Dir::L => Dir::U,
            Dir::U => Dir::R,
        };
        match r {
            Rot::R0 => d,
            Rot::R90 => cw(d),
            Rot::R180 => cw(cw(d)),
            Rot::R270 => cw(cw(cw(d))),
        }
    }

    fn collinear(a: Pt, b: Pt, c: Pt) -> bool {
        let (abx, aby) = (i64::from(b.x) - i64::from(a.x), i64::from(b.y) - i64::from(a.y));
        let (bcx, bcy) = (i64::from(c.x) - i64::from(b.x), i64::from(c.y) - i64::from(b.y));
        abx * bcy == aby * bcx
    }

    /// Canonical form of a polyline: repeatedly drop zero-length segments and every interior point collinear with
    /// both neighbours (this also removes spikes), until nothing changes. End points are kept.
    pub fn canonical(path: &[Pt]) -> Vec<Pt> {
        let mut pts = path.to_vec();
        loop {
            let mut changed = false;
            let mut i = 0;
            while pts.len() > 2 && i + 1 < pts.len() {
                if pts[i] == pts[i + 1] {
                    // Keep the end points: drop the interior one of the pair.
                    pts.remove(if i + 1 == pts.len() - 1 { i } else { i + 1 });
                    changed = true;
                } else {
                    i += 1;
                }
            }
            if let Some(i) = (1..pts.len().saturating_sub(1)).find(|&i| collinear(pts[i - 1], pts[i], pts[i + 1])) {
                pts.remove(i);
                changed = true;
            }
            if !changed {
                return pts;
            }
        }
    }

    /// Every segment horizontal or vertical, with positive length.
    pub fn is_orthogonal(path: &[Pt]) -> bool {
        path.len() >= 2 && path.windows(2).all(|s| s[0] != s[1] && (s[0].x == s[1].x || s[0].y == s[1].y))
    }

    fn step(p: Pt, d: Dir, n: i32) -> Pt {
        match d {
            Dir::L => Pt { x: p.x.saturating_sub(n), y: p.y },
            Dir::R => Pt { x: p.x.saturating_add(n), y: p.y },
            Dir::U => Pt { x: p.x, y: p.y.saturating_sub(n) },
            Dir::D => Pt { x: p.x, y: p.y.saturating_add(n) },
        }
    }

    fn horizontal(d: Dir) -> bool {
        matches!(d, Dir::L | Dir::R)
    }

    fn mid(a: i32, b: i32) -> i32 {
        (i64::from(a) + (i64::from(b) - i64::from(a)).div_euclid(2)) as i32
    }

    /// Candidate bend lists from `a` (lead direction `da`) to `b` (`db`), in this fixed order: straight, L via
    /// (b.x, a.y), L via (a.x, b.y), Z horizontal, Z vertical, U with 2-unit stubs along `da` and `db`. Candidates
    /// that are not orthogonal are omitted. The first one that passes validation is used.
    pub fn autoroute(a: Pt, da: Dir, b: Pt, db: Dir) -> Vec<Vec<Pt>> {
        let xm = mid(a.x, b.x);
        let ym = mid(a.y, b.y);
        let (a2, b2) = (step(a, da, 2), step(b, db, 2));
        // U: both stubs pushed to the outer line when parallel and alike, an S around the middle line when
        // opposite, one corner when perpendicular.
        let u: Vec<Pt> = if da == db {
            match da {
                Dir::U => vec![Pt { x: a.x, y: a2.y.min(b2.y) }, Pt { x: b.x, y: a2.y.min(b2.y) }],
                Dir::D => vec![Pt { x: a.x, y: a2.y.max(b2.y) }, Pt { x: b.x, y: a2.y.max(b2.y) }],
                Dir::L => vec![Pt { x: a2.x.min(b2.x), y: a.y }, Pt { x: a2.x.min(b2.x), y: b.y }],
                Dir::R => vec![Pt { x: a2.x.max(b2.x), y: a.y }, Pt { x: a2.x.max(b2.x), y: b.y }],
            }
        } else if horizontal(da) == horizontal(db) {
            if horizontal(da) {
                vec![a2, Pt { x: a2.x, y: ym }, Pt { x: b2.x, y: ym }, b2]
            } else {
                vec![a2, Pt { x: xm, y: a2.y }, Pt { x: xm, y: b2.y }, b2]
            }
        } else if horizontal(da) {
            vec![a2, Pt { x: a2.x, y: b2.y }, b2]
        } else {
            vec![a2, Pt { x: b2.x, y: a2.y }, b2]
        };
        let candidates: [Vec<Pt>; 6] = [
            Vec::new(),
            vec![Pt { x: b.x, y: a.y }],
            vec![Pt { x: a.x, y: b.y }],
            vec![Pt { x: xm, y: a.y }, Pt { x: xm, y: b.y }],
            vec![Pt { x: a.x, y: ym }, Pt { x: b.x, y: ym }],
            u,
        ];
        let mut out: Vec<Vec<Pt>> = Vec::new();
        for bends in candidates {
            let mut path = Vec::with_capacity(bends.len() + 2);
            path.push(a);
            path.extend(bends);
            path.push(b);
            let path = canonical(&path);
            if !is_orthogonal(&path) {
                continue;
            }
            let bends = path[1..path.len() - 1].to_vec();
            if !out.contains(&bends) {
                out.push(bends);
            }
        }
        out
    }
}

// ----------------------------------------------------------------------------------------------------- library

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DcSupply,
    Ground,
    Resistor,
    Capacitor,
    Potentiometer,
    PushButton,
    Switch,
    AdcInput,
    EcuStm32,
    CanTransceiver,
    CanBus,
    Termination,
    /// Synthetic vehicle ECU of the APPS mission, as a black box (library version 2).
    EcuApps,
    /// Dual-die ratiometric position sensor with its pedal travel (library version 2).
    Mlx90377,
    /// Mission ECU and sensors; model parameters are loaded from the mission bench.
    EcuWsf,
    EcuSas,
    EcuBps,
    #[serde(rename = "ats682lsh")]
    Ats682Lsh,
    Tle5014,
    Pss260,
    PressureSwitch,
    /// Mechanical inputs of the three mission benches (no electrical pins).
    FrontAxle,
    SteeringColumn,
    BrakeHydraulics,
    /// Created only by an edit endpoint on a wire (`Endpoint::OnWire`), removed by the cleanup.
    Junction,
}

impl Kind {
    /// First library version that knows this kind: a file of an older library version cannot hold it.
    pub fn since_library(self) -> u32 {
        match self {
            Kind::EcuApps | Kind::Mlx90377 => 2,
            Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps | Kind::Ats682Lsh | Kind::Tle5014
            | Kind::Pss260 | Kind::PressureSwitch | Kind::FrontAxle | Kind::SteeringColumn
            | Kind::BrakeHydraulics => 3,
            _ => 1,
        }
    }
}

/// Electrical role of a pin (architecture §9.1), used by diagnostics and by the inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Passive,
    Source,
    /// Pin of the global ground symbol: all of them form the net "GND".
    Ground,
    PowerIn,
    /// Local ground of an active component (ECU GND, transceiver VSS, ADC reference): never connected implicitly.
    GroundPin,
    AnalogIn,
    LogicIn,
    LogicOut,
    Canh,
    Canl,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PinDef {
    pub id: &'static str,
    /// Full name for the inspector pinout, e.g. "PD1 CAN_TX (CN9-27)".
    pub name: &'static str,
    /// Short name drawn on the canvas, e.g. "PD1".
    pub short: &'static str,
    /// Connection point at R0, relative to the component origin.
    pub pos: Pt,
    /// Lead direction at R0.
    pub dir: Dir,
    pub role: Role,
    /// Unconnected required pin ⇒ diagnostic `unconnected_pin`.
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Number,
    /// A finite number in [min, max], or +∞ (JSON "inf"): the ideal ADC input.
    NumberOrInf,
    Int,
    Bool,
    Choice,
}

/// A parameter or state value. JSON: number, integer, boolean or string; +∞ is written "inf" (only for
/// NumberOrInf parameters; NaN and −∞ never exist). A Number parameter accepts an integer (converted exactly) or a
/// text with the unit grammar of contract §4, parsed in Rust (refusals `param_parse`, `param_unit`); an Int
/// parameter accepts a number only if it is integral. Canonical files write Number values as floats (`10000.0`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Num(f64),
    Choice(String),
}

fn serialize_num<S: serde::Serializer>(x: f64, s: S) -> Result<S::Ok, S::Error> {
    if x == f64::INFINITY {
        s.serialize_str("inf")
    } else {
        s.serialize_f64(x)
    }
}

impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Int(i) => s.serialize_i64(*i),
            Value::Num(x) => serialize_num(*x, s),
            Value::Choice(c) => s.serialize_str(c),
        }
    }
}

pub type ValueMap = BTreeMap<String, Value>;

/// Constant value of the library (defaults), same JSON as [`Value`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lit {
    Bool(bool),
    Int(i64),
    Num(f64),
    Choice(&'static str),
}

impl Serialize for Lit {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(s)
    }
}

impl Lit {
    pub fn to_value(self) -> Value {
        match self {
            Lit::Bool(b) => Value::Bool(b),
            Lit::Int(i) => Value::Int(i),
            Lit::Num(x) => Value::Num(x),
            Lit::Choice(s) => Value::Choice(s.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ParamDef {
    pub name: &'static str,
    pub label_fr: &'static str,
    /// SI symbol ("V", "A", "Ω" written U+03A9, "F", "W", "Hz", "s") or "" for dimensionless.
    pub unit: &'static str,
    pub vtype: ValueType,
    /// Bounds for Number, NumberOrInf (finite part) and Int, inclusive.
    pub min: f64,
    pub max: f64,
    pub default: Lit,
    /// Allowed values for Choice.
    pub choices: &'static [&'static str],
    /// false: fixed by the part (refusal `param_locked`). Every editable parameter can be edited in RUN.
    pub editable: bool,
    /// Internal model value of a black box or of a mission part: kept in the document and in saved files, never
    /// sent to the UI (library, snapshot, patches), never editable (refusal `param_hidden`). Catalog defaults are
    /// neutral; real values come only from a mission bench file.
    #[serde(skip)]
    pub hidden: bool,
}

/// The parameters the UI may see: hidden ones are left out of every serialization.
fn visible_params<S: serde::Serializer>(params: &&'static [ParamDef], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(params.iter().filter(|p| !p.hidden))
}

/// Interaction state (pot position, contact, power): changed by [`Interact`], journaled, outside undo.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StateDef {
    pub name: &'static str,
    pub label_fr: &'static str,
    pub unit: &'static str,
    pub vtype: ValueType,
    pub min: f64,
    pub max: f64,
    pub default: Lit,
    /// false: transient (a held push button), never saved, default value after a load.
    pub persistent: bool,
}

/// Version of the library data, written in every saved circuit. Version 2 adds `ecu_apps`, `mlx90377` and hidden
/// parameters; version-1 files still load as they are (they cannot hold the new kinds). Any other value refuses
/// the file (`library_mismatch`), without implicit migration.
pub const LIBRARY_VERSION: u32 = 3;

/// Library versions whose files load.
pub const LIBRARY_VERSIONS_READ: [u32; 3] = [1, 2, 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActuatorKind {
    /// Click toggles `closed`.
    ToggleSwitch,
    /// Pressed on pointerdown, released on pointerup / pointercancel / blur.
    PressButton,
    /// Drag maps the pointer to `position` along the A→B axis.
    DragPot,
}

/// Zone of the symbol (R0 coordinates) where a pointer actuates instead of selecting.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Actuator {
    pub kind: ActuatorKind,
    pub zone: Rect,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KindDef {
    pub kind: Kind,
    pub name_fr: &'static str,
    /// Designator prefix ("R", "RV", "U"); "" for ground and junction (no designator).
    pub prefix: &'static str,
    /// Symbol body at R0, leads excluded: wires may not cross it (ambiguity `wire_through_body`).
    pub body: Rect,
    pub pins: &'static [PinDef],
    /// Every parameter, hidden ones included; serialized without the hidden ones.
    #[serde(serialize_with = "visible_params")]
    pub params: &'static [ParamDef],
    pub states: &'static [StateDef],
    pub actuators: &'static [Actuator],
    /// false for the junction (AddComponent refuses it with `unknown_kind`).
    pub placeable: bool,
    /// Provenance of the model values (datasheet references), shown in the inspector.
    pub sources_fr: &'static str,
}

/// The V1 library, in palette order (contract §2): data in `catalog.rs`, part of the contract.
pub fn library() -> &'static [KindDef] {
    &catalog::LIBRARY
}

pub fn kind_def(kind: Kind) -> &'static KindDef {
    catalog::LIBRARY.iter().find(|d| d.kind == kind).expect("every kind is in the library")
}

// ------------------------------------------------------------------------------------------------------- model

/// JSON (UI DTOs, patches): `params` without the hidden parameters of the kind. Canonical files are written by
/// [`persist::save`], which keeps them.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Component {
    pub id: CompId,
    pub kind: Kind,
    /// Designator, unique in the circuit, `[A-Za-z][A-Za-z0-9_]{0,15}`; "" for ground and junction.
    pub label: String,
    pub pos: Pt,
    pub rot: Rot,
    /// Every parameter of the kind, defaults filled, all within range (hidden ones included).
    pub params: ValueMap,
}

impl Serialize for Component {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeStruct};
        struct Visible<'a>(&'a Component);
        impl Serialize for Visible<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let hidden = |name: &str| kind_def(self.0.kind).params.iter().any(|p| p.hidden && p.name == name);
                let mut m = s.serialize_map(None)?;
                for (k, v) in self.0.params.iter().filter(|(k, _)| !hidden(k)) {
                    m.serialize_entry(k, v)?;
                }
                m.end()
            }
        }
        let mut st = s.serialize_struct("Component", 6)?;
        st.serialize_field("id", &self.id)?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("label", &self.label)?;
        st.serialize_field("pos", &self.pos)?;
        st.serialize_field("rot", &self.rot)?;
        st.serialize_field("params", &Visible(self))?;
        st.end()
    }
}

/// Wire between two pins. Path = [pin a, bends…, pin b]: canonical and orthogonal (contract §3.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: WireId,
    pub a: PinRef,
    pub b: PinRef,
    pub bends: Vec<Pt>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Net {
    pub key: NetKey,
    /// "GND" for the ground net; otherwise "<designator>.<short pin name>" of its smallest non-junction pin
    /// ("R1.2"); "J<id>" if it holds only junctions.
    pub name: String,
    pub ground: bool,
    /// Ascending.
    pub pins: Vec<PinRef>,
    /// Ascending.
    pub wires: Vec<WireId>,
}

/// Nets by union-find over wires; all pins of role `ground` are one net. Nets in ascending key order; every pin of
/// every component belongs to exactly one net (an unconnected pin is a net of one pin).
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct Netlist {
    pub nets: Vec<Net>,
}

impl Netlist {
    pub fn net_of(&self, pin: &PinRef) -> Option<&Net> {
        self.nets.iter().find(|n| n.pins.binary_search(pin).is_ok())
    }
}

/// A broken invariant (load validation, property tests).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Violation {
    pub code: String,
    pub message_fr: String,
    pub items: Vec<ItemRef>,
    pub at: Option<Pt>,
}

/// The document. Mutated only through [`Document`] (edits, undo/redo, interactions) or [`persist::load`].
#[derive(Clone, Debug, PartialEq)]
pub struct Circuit {
    name: String,
    next_id: u32,
    components: BTreeMap<CompId, Component>,
    wires: BTreeMap<WireId, Wire>,
    /// Interaction states, including deleted components still reachable through the document's undo/redo history.
    states: BTreeMap<CompId, ValueMap>,
}

impl Circuit {
    pub fn new(name: &str) -> Self {
        Circuit {
            name: name.to_string(),
            next_id: 1,
            components: BTreeMap::new(),
            wires: BTreeMap::new(),
            states: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    pub fn components(&self) -> &BTreeMap<CompId, Component> {
        &self.components
    }

    pub fn wires(&self) -> &BTreeMap<WireId, Wire> {
        &self.wires
    }

    /// Interaction state of an existing component, every state of its kind present.
    pub fn state(&self, id: CompId) -> Option<&ValueMap> {
        if self.components.contains_key(&id) {
            self.states.get(&id)
        } else {
            None
        }
    }

    /// World position of a pin's connection point.
    pub fn pin_pos(&self, pin: &PinRef) -> Option<Pt> {
        model::pin_pos(&self.components, pin)
    }

    /// World lead direction of a pin.
    pub fn pin_dir(&self, pin: &PinRef) -> Option<Dir> {
        model::pin_dir(&self.components, pin)
    }

    /// Full path of a wire: [pin a, bends…, pin b].
    pub fn wire_path(&self, id: WireId) -> Option<Vec<Pt>> {
        model::path_of(&self.components, self.wires.get(&id)?)
    }

    /// Rotated body rectangle in world coordinates.
    pub fn body_rect(&self, id: CompId) -> Option<Rect> {
        self.components.get(&id).map(model::body_rect)
    }

    pub fn netlist(&self) -> Netlist {
        netlist::build(&self.components, &self.wires)
    }

    /// Nets as if the wires of `cut` did not conduct (hidden wire defects of the mission engine: the document and
    /// its nets stay those of healthy wires).
    pub fn netlist_excluding(&self, cut: &BTreeSet<WireId>) -> Netlist {
        if cut.is_empty() {
            return self.netlist();
        }
        let mut wires = self.wires.clone();
        wires.retain(|id, _| !cut.contains(id));
        netlist::build(&self.components, &wires)
    }

    /// Every invariant of contract §3.5 (ids, kinds, params in range, labels, canonical orthogonal paths, endpoints,
    /// junction degrees, no duplicate pin pair, no ambiguity). Empty = valid.
    pub fn check(&self) -> Vec<Violation> {
        check::check(self)
    }

    /// Applies a patch (forward); a component that appears gets its last known interaction state, or the defaults.
    fn apply(&mut self, patch: &Patch) {
        for (id, _, after) in &patch.components {
            match after {
                Some(c) => {
                    self.states.entry(*id).or_insert_with(|| model::default_states(c.kind));
                    self.components.insert(*id, c.clone());
                }
                None => {
                    self.components.remove(id);
                }
            }
        }
        for (id, _, after) in &patch.wires {
            match after {
                Some(w) => {
                    self.wires.insert(*id, w.clone());
                }
                None => {
                    self.wires.remove(id);
                }
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------------- edits

/// Where a wire end goes. `OnWire` splits that wire at `at` with a new junction (contract §3.4.6); an `at` equal to
/// one of the wire's ends means that end's pin. A junction has at most 3 wires (`four_way_junction`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Endpoint {
    Pin { comp: CompId, pin: String },
    OnWire { wire: WireId, at: Pt },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    A,
    B,
}

/// An undoable transaction. One gesture = one edit (sent at pointerup). JSON tagged by "op".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// New component with defaults overridden by `params`; label = prefix + smallest free number unless given.
    AddComponent {
        kind: Kind,
        pos: Pt,
        rot: Rot,
        #[serde(default)]
        params: ValueMap,
        #[serde(default)]
        label: Option<String>,
    },
    /// Deletes the components with their wires, and the wires; selected junctions are skipped (the cleanup removes
    /// them). Then junction cleanup.
    Delete { components: Vec<CompId>, wires: Vec<WireId> },
    /// Moved set rules and end repair of contract §3.4.3.
    Move { components: Vec<CompId>, wires: Vec<WireId>, delta: Pt },
    /// +90°: a single component about its origin, a selection about the snapped centre of its bounding box;
    /// attached wires repaired.
    Rotate { components: Vec<CompId>, wires: Vec<WireId> },
    /// `value` typed, or a text parsed in Rust for Number / NumberOrInf parameters (contract §4).
    SetParam { comp: CompId, name: String, value: Value },
    SetLabel { comp: CompId, label: String },
    /// `bends` None: first valid autoroute candidate.
    AddWire {
        a: Endpoint,
        b: Endpoint,
        #[serde(default)]
        bends: Option<Vec<Pt>>,
    },
    /// Reattach one end; an abandoned junction goes through the cleanup.
    MoveWireEnd {
        wire: WireId,
        end: End,
        to: Endpoint,
        #[serde(default)]
        bends: Option<Vec<Pt>>,
    },
    /// Reshape (segment drag); the bends are canonicalized.
    SetBends { wire: WireId, bends: Vec<Pt> },
}

/// A manipulation of the running circuit: journaled, never in undo/redo. JSON tagged by "op".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Interact {
    /// Mechanical quantity of a mission bench, validated against its public state definition.
    SetWorld { comp: CompId, quantity: String, value: f64 },
    /// Wiper position α ∈ [0, 1], 0 = wiper at A, or pedal travel of a position sensor (0 = rest); quantized to
    /// 1e-3.
    SetPot { comp: CompId, position: f64 },
    /// Push button actuator.
    SetButton { comp: CompId, pressed: bool },
    SetSwitch { comp: CompId, closed: bool },
    /// DC supply output on/off, ECU board powered (USB) or not.
    SetPower { comp: CompId, on: bool },
}

/// Exact diff of the components and wires maps: every id whose value differs, cascades included, as
/// (id, before, after) with `None` = absent; ascending ids. JSON `[[id, before|null, after|null], …]`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Patch {
    pub components: Vec<(CompId, Option<Component>, Option<Component>)>,
    pub wires: Vec<(WireId, Option<Wire>, Option<Wire>)>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.wires.is_empty()
    }

    /// The patch that undoes this one.
    pub fn inverse(&self) -> Patch {
        Patch {
            components: self.components.iter().map(|(id, b, a)| (*id, a.clone(), b.clone())).collect(),
            wires: self.wires.iter().map(|(id, b, a)| (*id, a.clone(), b.clone())).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemRef {
    Component { id: CompId },
    Wire { id: WireId },
    Pin { comp: CompId, pin: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    UnknownComponent,
    UnknownWire,
    UnknownPin,
    /// Unknown kind, or a kind that cannot be placed (junction).
    UnknownKind,
    SamePin,
    /// A wire from a wire to itself.
    SameWire,
    DuplicateWire,
    NonOrthogonal,
    OutOfBounds,
    /// No autoroute candidate is valid.
    NoRoute,
    ParamUnknown,
    ParamType,
    /// Text value that does not follow the unit grammar of contract §4.
    ParamParse,
    /// Text value with a unit other than the parameter's.
    ParamUnit,
    ParamOutOfRange,
    ParamLocked,
    /// Internal parameter of a black box or of a mission part: never shown, never edited.
    ParamHidden,
    /// Refused while a mission bench is the current document (« Enregistrement indisponible en mission. »).
    MissionLocked,
    /// Refused while a mission check drives the bench (« Banc en lecture seule pendant l'essai. »).
    BenchLocked,
    /// The mission bus owns the physical clock; UI clock controls cannot reset its timeline.
    SourceClockOwned,
    /// A command of the mission engine sent from the interface (« Commande réservée au moteur de mission. »).
    InternalOnly,
    StateUnknown,
    StateOutOfRange,
    /// The component has no such interaction.
    NotInteractive,
    LabelInvalid,
    LabelDuplicate,
    EndpointNotOnWire,
    JunctionOnEndpoint,
    /// A fourth wire on a junction (« Jonction à 4 branches interdite : décalez en deux T »).
    FourWayJunction,
    /// Sub-reason in [`Refusal::ambiguity`].
    Ambiguous,
    /// base_rev ≠ current revision: the UI reloads the snapshot.
    StaleRevision,
    NothingToUndo,
    NothingToRedo,
}

/// Why a geometry is ambiguous (contract §3.4): what looks connected must be, nothing may look connected without
/// being.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ambiguity {
    PinOnPin,
    PinOnWire,
    WireOverPin,
    CollinearOverlap,
    /// A wire overlapping or touching itself.
    SelfOverlap,
    BendOnWire,
    WireThroughBody,
}

/// A refused edit or interaction: nothing changed. `message_fr` is shown as is; `at` and `items` let the preview
/// mark the conflict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    pub code: RefusalCode,
    pub ambiguity: Option<Ambiguity>,
    pub message_fr: String,
    pub at: Option<Pt>,
    pub items: Vec<ItemRef>,
}

/// Result of a committed edit, undo or redo.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Applied {
    pub rev: u64,
    pub patch: Patch,
    /// Connectivity changed (nets must be recomputed and the solver network rebuilt).
    pub topology_changed: bool,
    /// Something electrical changed (topology or a parameter); false for pure geometry (move, rotate, bends,
    /// label), which never commutes the solver.
    pub electrical_changed: bool,
}

/// Result of an interaction.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StateChange {
    /// Current (unchanged) document revision.
    pub rev: u64,
    pub comp: CompId,
    /// Full state of the component after the interaction.
    pub state: ValueMap,
}

/// Circuit + revision + undo history. `rev` increases on every committed edit, undo, redo and load; interactions
/// do not change it (they are manipulations, not edits, and must not make a queued edit stale). Empty patches are
/// not committed (no new rev, no history entry).
#[derive(Clone, Debug)]
pub struct Document {
    circuit: Circuit,
    rev: u64,
    /// Committed patches, oldest first.
    undo: VecDeque<Patch>,
    /// Undone patches, the next one to redo last.
    redo: Vec<Patch>,
}

/// Undo entries kept at most (oldest dropped).
pub const UNDO_CAP: usize = 200;

/// (topology_changed, electrical_changed) of a patch: connectivity (an item appears or disappears, a wire changes
/// ends) or a parameter; positions, rotations, bends and labels are geometry only.
fn patch_effects(patch: &Patch) -> (bool, bool) {
    let mut topology = false;
    let mut params = false;
    for (_, before, after) in &patch.components {
        match (before, after) {
            (Some(b), Some(a)) => params |= b.params != a.params,
            _ => topology = true,
        }
    }
    for (_, before, after) in &patch.wires {
        match (before, after) {
            (Some(b), Some(a)) => topology |= b.a != a.a || b.b != a.b,
            _ => topology = true,
        }
    }
    (topology, topology || params)
}

fn stale(rev: u64, base_rev: u64) -> Result<(), Refusal> {
    if base_rev == rev {
        return Ok(());
    }
    Err(model::refusal(
        RefusalCode::StaleRevision,
        format!("Le sch\u{e9}ma a chang\u{e9} entre-temps (r\u{e9}vision {base_rev}, actuelle {rev}) : rechargement"),
    ))
}

impl Document {
    pub fn new(circuit: Circuit) -> Self {
        let mut document = Document { circuit, rev: 0, undo: VecDeque::new(), redo: Vec::new() };
        document.prune_states();
        document
    }

    pub fn circuit(&self) -> &Circuit {
        &self.circuit
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Dry run with the same function as [`Document::edit`]: never committed, never journaled, consumes no id
    /// (created items get ids from next_id without advancing it).
    pub fn preview(&self, base_rev: u64, edit: &Edit) -> Result<Patch, Refusal> {
        stale(self.rev, base_rev)?;
        Ok(edit::run(&self.circuit, edit)?.patch(&self.circuit))
    }

    pub fn edit(&mut self, base_rev: u64, edit: &Edit) -> Result<Applied, Refusal> {
        stale(self.rev, base_rev)?;
        let st = edit::run(&self.circuit, edit)?;
        let patch = st.patch(&self.circuit);
        if patch.is_empty() {
            return Ok(Applied { rev: self.rev, patch, topology_changed: false, electrical_changed: false });
        }
        self.circuit.apply(&patch);
        self.circuit.next_id = st.next_id;
        self.rev += 1;
        if self.undo.len() == UNDO_CAP {
            self.undo.pop_front();
        }
        self.undo.push_back(patch.clone());
        self.redo.clear();
        self.prune_states();
        let (topology_changed, electrical_changed) = patch_effects(&patch);
        Ok(Applied { rev: self.rev, patch, topology_changed, electrical_changed })
    }

    /// A change made by the mission engine (the setting of an incident): validated and applied like [`Document::edit`]
    /// on the current revision, then the undo and redo history is cleared, so that no older entry (which holds whole
    /// components before and after) can revert it.
    pub fn system_edit(&mut self, edit: &Edit) -> Result<Applied, Refusal> {
        let applied = self.edit(self.rev, edit)?;
        self.undo.clear();
        self.redo.clear();
        Ok(applied)
    }

    /// Applies the inverse of the last entry; interaction states untouched (a re-created component gets its last
    /// known state).
    pub fn undo(&mut self) -> Result<Applied, Refusal> {
        let Some(patch) = self.undo.pop_back() else {
            return Err(model::refusal(RefusalCode::NothingToUndo, "Rien \u{e0} annuler"));
        };
        let inverse = patch.inverse();
        self.circuit.apply(&inverse);
        self.redo.push(patch);
        self.prune_states();
        self.rev += 1;
        let (topology_changed, electrical_changed) = patch_effects(&inverse);
        Ok(Applied { rev: self.rev, patch: inverse, topology_changed, electrical_changed })
    }

    pub fn redo(&mut self) -> Result<Applied, Refusal> {
        let Some(patch) = self.redo.pop() else {
            return Err(model::refusal(RefusalCode::NothingToRedo, "Rien \u{e0} r\u{e9}tablir"));
        };
        self.circuit.apply(&patch);
        if self.undo.len() == UNDO_CAP {
            self.undo.pop_front();
        }
        self.undo.push_back(patch.clone());
        self.prune_states();
        self.rev += 1;
        let (topology_changed, electrical_changed) = patch_effects(&patch);
        Ok(Applied { rev: self.rev, patch, topology_changed, electrical_changed })
    }

    pub fn interact(&mut self, interact: &Interact) -> Result<StateChange, Refusal> {
        let (comp, kinds, what): (CompId, &[Kind], &str) = match interact {
            Interact::SetWorld { comp, .. } => (*comp, &[Kind::FrontAxle, Kind::SteeringColumn, Kind::BrakeHydraulics], "n'a pas de commande mécanique"),
            Interact::SetPot { comp, .. } => (*comp, &[Kind::Potentiometer, Kind::Mlx90377], "n'a pas de curseur"),
            Interact::SetButton { comp, .. } => (*comp, &[Kind::PushButton], "n'est pas un bouton poussoir"),
            Interact::SetSwitch { comp, .. } => (*comp, &[Kind::Switch], "n'est pas un interrupteur"),
            Interact::SetPower { comp, .. } => {
                (*comp, &[Kind::DcSupply, Kind::EcuStm32], "n'a pas d'alimentation \u{e0} commuter")
            }
        };
        let item = vec![ItemRef::Component { id: comp }];
        let Some(c) = self.circuit.components.get(&comp) else {
            return Err(Refusal {
                items: item,
                ..model::refusal(RefusalCode::UnknownComponent, format!("Composant {} inexistant", comp.0))
            });
        };
        if !kinds.contains(&c.kind) {
            return Err(Refusal {
                items: item,
                ..model::refusal(RefusalCode::NotInteractive, format!("{} ({}) {what}", model::comp_name(c), kind_def(c.kind).name_fr))
            });
        }
        let kind = c.kind;
        let mut state = self.circuit.states.get(&comp).cloned().unwrap_or_else(|| model::default_states(kind));
        match interact {
            Interact::SetWorld { quantity, value, .. } => {
                let def = kind_def(kind).states.iter().find(|s| s.name == quantity)
                    .ok_or_else(|| model::refusal(RefusalCode::NotInteractive, "Commande mécanique inconnue"))?;
                let comp_params = &self.circuit.components[&comp].params;
                let param = |name: &str, fallback: f64| match comp_params.get(name) {
                    Some(Value::Num(v)) => *v, Some(Value::Int(v)) => *v as f64, _ => fallback,
                };
                let (lo,hi) = if kind == Kind::SteeringColumn && quantity == "angle_deg" {
                    (param("min_angle_deg",def.min).max(def.min), param("max_angle_deg",def.max).min(def.max))
                } else if kind == Kind::BrakeHydraulics && quantity == "pressure_bar" {
                    (param("min_pressure_bar",def.min).max(def.min),param("max_pressure_bar",def.max).min(def.max))
                } else { (def.min,def.max) };
                if !value.is_finite() || lo > hi || !(lo..=hi).contains(value) {
                    return Err(model::refusal(RefusalCode::StateOutOfRange, "Commande mécanique hors de sa plage"));
                }
                state.insert(quantity.clone(), Value::Num((value * 1000.0).round() / 1000.0));
            }
            Interact::SetPot { position, .. } => {
                if !(position.is_finite() && (0.0..=1.0).contains(position)) {
                    return Err(Refusal {
                        items: item,
                        ..model::refusal(
                            RefusalCode::StateOutOfRange,
                            units::out_of_range(&format!("Position {}", units::quantity(*position, "")), "0", "1"),
                        )
                    });
                }
                // α = k/1000, k integral (contract §2.1).
                let k = (position * 1000.0).round() as i64;
                state.insert("position".to_string(), Value::Num(k as f64 / 1000.0));
            }
            Interact::SetButton { pressed, .. } => {
                let was = matches!(state.get("pressed"), Some(Value::Bool(true)));
                if *pressed && !was {
                    let latched = matches!(state.get("latched"), Some(Value::Bool(true)));
                    state.insert("latched".to_string(), Value::Bool(!latched));
                }
                state.insert("pressed".to_string(), Value::Bool(*pressed));
            }
            Interact::SetSwitch { closed, .. } => {
                state.insert("closed".to_string(), Value::Bool(*closed));
            }
            Interact::SetPower { on, .. } => {
                let name = if kind == Kind::DcSupply { "on" } else { "usb" };
                state.insert(name.to_string(), Value::Bool(*on));
            }
        }
        self.circuit.states.insert(comp, state.clone());
        Ok(StateChange { rev: self.rev, comp, state })
    }

    /// Keep the last interaction state for every component that a retained undo/redo patch can restore.
    /// History eviction and a new edit invalidating redo must release states that can no longer return.
    fn prune_states(&mut self) {
        let reachable: BTreeSet<_> = self.circuit.components.keys().copied().chain(
            self.undo.iter().chain(self.redo.iter()).flat_map(|patch| patch.components.iter().map(|(id, _, _)| *id)),
        ).collect();
        self.circuit.states.retain(|id, _| reachable.contains(id));
    }
}

#[cfg(test)]
#[path = "../tests/retention/circuit.rs"]
mod retention_tests;

// --------------------------------------------------------------------------------------------------- persistence

/// Canonical JSON (contract §7): `save(&load(t)?) == t` byte for byte for a canonical text, `load(&save(c))` equals
/// `c` (components, parameters, positions, rotations, labels, wires, bends, persistent interaction states, next_id).
pub mod persist {
    use super::{Circuit, Violation};

    pub const FORMAT: &str = "n7rt-physlab-circuit";
    pub const VERSION: u32 = 1;

    /// UTF-8 without BOM, LF, serde_json pretty (2 spaces) plus a final LF. Key order: root format, version,
    /// library_version, name, next_id, components, wires; component id, kind, label, pos, rot, params, state; wire
    /// id, a, b, bends. params and state sorted by key (transient states omitted); items by ascending id.
    pub fn save(circuit: &Circuit) -> String {
        crate::store::save(circuit)
    }

    /// Parses and validates every invariant ([`Circuit::check`]) plus format, version and library_version
    /// (`format_unknown`, `version_unsupported`, `library_mismatch`, `json_invalid`); never corrects anything;
    /// refuses the file with the full list.
    pub fn load(text: &str) -> Result<Circuit, Vec<Violation>> {
        crate::store::load(text)
    }
}

// ------------------------------------------------------------------------------------------------------- demos

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Demo {
    pub id: &'static str,
    pub title_fr: &'static str,
    pub summary_fr: &'static str,
    /// Hidden from the UI list (performance fixture).
    pub hidden: bool,
    /// Canonical circuit text, embedded with include_str!.
    pub json: &'static str,
}

/// Embedded demonstrations (contract §9): "ref-pot-rc", "tp1-capteur-can", and the hidden performance fixture
/// "perf-150-300" (≥ 150 non-junction components, ≥ 300 wires, ≥ 50 junctions, deterministic).
pub fn demos() -> &'static [Demo] {
    &demos::DEMOS
}

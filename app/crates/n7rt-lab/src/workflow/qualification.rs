//! Hidden final qualification E4 (private spec ch. 24 §6), instructor edition, generic: five segments (start,
//! nominal, limits, supervision, defects and recovery) rendered on the reference bench with the student projection,
//! then reshaped at the frame level. Their durations, the frame events, the defects drawn and the competence of every
//! comparison point come from the private truth file (`qualification.schedule`), never from this code. The vector
//! holds frames and instants only; the expected outputs come from the reference consumer (instructor edition only).

use std::collections::BTreeMap;

use n7rt_circuit::{persist, Interact, Kind};
use n7rt_vehicle::{e2e, BusFrame, Pack, Vehicle};
use serde::Deserialize;
use serde_json::Value;

use crate::phys::PhysCommand;
use crate::workflow::bench::VirtualBench;
use crate::workflow::compare::{Expected, COMPETENCES};
use crate::workflow::sequence::{self, Draw};

/// Displayed labels of the competences (ch. 24 §6.3, public, verbatim).
pub const LABELS: [&str; COMPETENCES] = [
    "Recevoir et filtrer les trames de l'ECU",
    "Décoder les valeurs nominales",
    "Respecter les limites et les valeurs invalides",
    "Contrôler l'intégrité des messages",
    "Superviser fraîcheur et séquence",
    "Traiter un défaut signalé par la source",
    "Appliquer la logique de sécurité de la mission",
    "Reprendre après un défaut",
];

/// The schedule of the hidden qualification (truth file, `qualification.schedule`): absolute instants in ms.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    duration_ms: u64,
    /// The start, nominal, limits and supervision segments, `[start, end)`.
    q0: (u64, u64),
    qn: (u64, u64),
    ql: (u64, u64),
    qs: (u64, u64),
    /// Starts of the three defects (one per category, in the drawn order), each `defect_ms` long, then as long again of
    /// recovery.
    defects_at_ms: [u64; 3],
    defect_ms: u64,
    /// How long a reaction belongs to the protection, after a wrong protection, and to the frame filter, after a
    /// foreign frame.
    after_protection_ms: u64,
    after_foreign_ms: u64,
    /// The main value rewritten at its codable maximum, above it, at its invalidity code.
    rewrites: [(u64, u64); 3],
    foreign: Vec<Foreign>,
    supervision: Supervision,
    /// The competence of a point: the first rule whose condition holds, else `otherwise`.
    labels: Vec<Rule>,
    otherwise: u8,
}

/// A frame the mission ECU does not send, added to the bus.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Foreign {
    at_ms: u64,
    kind: ForeignKind,
    /// The identifier of an `unknown` frame (absent from the variant's matrix).
    #[serde(default)]
    id: Option<Value>,
    /// The DLC of a `short` or `fd` frame.
    #[serde(default)]
    dlc: Option<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ForeignKind {
    /// Another standard identifier.
    Unknown,
    /// The DATA identifier as an extended one.
    Ext,
    /// The DATA identifier, remote frame.
    Rtr,
    /// The DATA identifier, CAN FD frame.
    Fd,
    /// The DATA identifier with a DLC the matrix does not give it.
    Short,
}

/// The supervision events: omissions, a delayed frame, two frames in one cycle, a repeated counter, a reset of the
/// source ECU (its supply cut on the bench), an isolated wrong protection.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Supervision {
    omit1_at_ms: u64,
    omit3_at_ms: u64,
    delay_at_ms: u64,
    delay_ms: u64,
    burst_at_ms: u64,
    burst_gap_ms: u64,
    repeat_at_ms: u64,
    reset_at_ms: u64,
    reset_ms: u64,
    wrong_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    when: When,
    cp: u8,
}

/// The conditions a labelling rule can name.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum When {
    /// After a wrong protection (supervision event, or a MESSAGE defect and after it).
    Protection,
    /// The start segment.
    Start,
    /// The supervision segment.
    Supervision,
    /// A SENSOR defect.
    Sensor,
    /// The recovery after any defect.
    Returns,
    /// A slot of the mission's safety logic, in the nominal segment or a SOURCE defect.
    SafetyLogic,
    /// The limits segment.
    Limits,
    /// After a foreign frame.
    Foreign,
    /// A value slot in the nominal segment.
    NominalValues,
    /// A SOURCE defect.
    Source,
}

impl Schedule {
    fn parse(q: &Value) -> Result<Schedule, String> {
        let s: Schedule = serde_json::from_value(q["schedule"].clone()).map_err(|_| bad("calendrier"))?;
        let ordered = |w: (u64, u64)| w.0 < w.1;
        let segments = [s.q0, s.qn, s.ql, s.qs];
        let chained = segments.windows(2).all(|w| w[0].1 <= w[1].0);
        let defects = s.defect_ms > 0
            && s.defects_at_ms.windows(2).all(|w| w[0] + 2 * s.defect_ms <= w[1])
            && s.defects_at_ms[0] >= s.qs.1
            && s.defects_at_ms[2] + 2 * s.defect_ms <= s.duration_ms;
        let instants = s.foreign.iter().map(|f| f.at_ms).chain(s.rewrites.iter().map(|w| w.1)).chain([
            s.supervision.omit1_at_ms,
            s.supervision.omit3_at_ms,
            s.supervision.delay_at_ms,
            s.supervision.burst_at_ms,
            s.supervision.repeat_at_ms,
            s.supervision.reset_at_ms + s.supervision.reset_ms,
            s.supervision.wrong_at_ms,
        ]);
        let within = instants.into_iter().all(|t| t < s.duration_ms);
        let cps = s.labels.iter().map(|r| r.cp).chain([s.otherwise]).all(|cp| (1..=COMPETENCES as u8).contains(&cp));
        let foreign = s.foreign.iter().all(|f| match f.kind {
            ForeignKind::Unknown => f.id.as_ref().and_then(num).is_some_and(|x| (0..=0x7FF).contains(&x)),
            ForeignKind::Short => f.dlc.is_some_and(|d| d <= 8),
            ForeignKind::Fd => f.dlc.map_or(true, |d| (9..=15).contains(&d)),
            ForeignKind::Ext | ForeignKind::Rtr => true,
        });
        if !(segments.iter().all(|w| ordered(*w)) && chained && defects && within && cps && foreign && !s.labels.is_empty()) {
            return Err(bad("calendrier"));
        }
        Ok(s)
    }

    fn unknown_ids(&self) -> Vec<u32> {
        self.foreign.iter().filter(|f| f.kind == ForeignKind::Unknown).filter_map(|f| f.id.as_ref().and_then(num)).map(|x| x as u32).collect()
    }
}

/// Payload length of a CAN FD DLC.
fn fd_length(dlc: u8) -> usize {
    match dlc {
        0..=8 => dlc as usize,
        9 => 12,
        10 => 16,
        11 => 20,
        12 => 24,
        13 => 32,
        14 => 48,
        _ => 64,
    }
}

fn bad(what: &str) -> String {
    format!("qualification invalide : {what}")
}

fn in_window(t: u64, (a, b): (u64, u64)) -> bool {
    t >= a && t < b
}

/// The layout of a signal of the mission's DATA frame in the variant: positions least significant first, raw
/// maximum, invalidity code, enum name.
struct Field {
    positions: Vec<u16>,
    max: i64,
    sna: Option<i64>,
    enumeration: Option<String>,
}

fn num(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => {
            let t = s.trim();
            match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => i64::from_str_radix(h, 16).ok(),
                None => t.parse().ok(),
            }
        }
        _ => None,
    }
}

fn positions(start: u16, len: u8, motorola: bool) -> Vec<u16> {
    if !motorola {
        return (0..len as u16).map(|i| start + i).collect();
    }
    let mut msb_first = Vec::with_capacity(len as usize);
    let mut p = start as i32;
    for i in 0..len {
        msb_first.push(p.max(0) as u16);
        if i + 1 < len {
            p = if p % 8 != 0 { p - 1 } else { p + 15 };
        }
    }
    msb_first.reverse();
    msb_first
}

/// The DATA frame (end-to-end protected) of the mission ECU carrying `source`, and the field.
fn data_field(doc: &Value, source: &str) -> Result<(u32, Field), String> {
    let emitter = doc["source_ecu"]["emitter"].as_str().ok_or_else(|| bad("émetteur"))?;
    for f in doc["frames"].as_array().ok_or_else(|| bad("trames"))? {
        if f["emitter"].as_str() != Some(emitter) || f["protection"]["kind"].as_str() != Some("p22") {
            continue;
        }
        for s in f["signals"].as_array().ok_or_else(|| bad("signaux"))? {
            if s["source"].as_str() != Some(source) {
                continue;
            }
            let start = s["start"].as_u64().filter(|x| *x < 64).ok_or_else(|| bad("position"))? as u16;
            let len = s["len"].as_u64().filter(|x| (1..=32).contains(x)).ok_or_else(|| bad("longueur"))? as u8;
            let motorola = s["order"].as_str() == Some("motorola");
            let id = num(&f["id"]).filter(|x| (0..=0x7FF).contains(x)).ok_or_else(|| bad("identifiant"))? as u32;
            let max = num(&s["max"]).unwrap_or((1i64 << len) - 1);
            return Ok((id, Field { positions: positions(start, len, motorola), max, sna: s.get("sna").and_then(num), enumeration: s["enum"].as_str().map(str::to_owned) }));
        }
    }
    Err(bad("signal de la trame DATA introuvable"))
}

fn write_raw(data: &mut [u8], positions: &[u16], raw: i64) {
    for (i, p) in positions.iter().enumerate() {
        let (byte, bit) = (*p as usize / 8, p % 8);
        if let Some(b) = data.get_mut(byte) {
            if (raw >> i) & 1 == 1 {
                *b |= 1 << bit;
            } else {
                *b &= !(1 << bit);
            }
        }
    }
}

/// How a category's defect is rendered in the frames (truth file names, resolved on the variant).
#[derive(Clone, Debug, PartialEq)]
enum Render {
    /// Status at `code` and the value at its invalidity code.
    StatusSna { status: Vec<u16>, code: i64 },
    /// The application frozen (DC-M21): the last DATA and STATUS frames sent again unchanged, counters included.
    Frozen,
    /// The value above its range under an OK status.
    OutOfRange,
    /// The protection broken on every frame of the window.
    Corrupt,
    /// The frames protected with another Data ID list.
    WrongDataIds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Sensor,
    Source,
    Message,
}

/// The resolved qualification of a mission for one hidden test.
struct Plan {
    s: Schedule,
    drawn: sequence::Drawn,
    data_id: u32,
    data_ids: [u8; 16],
    /// The source ECU's alive-protected frames (STATUS), frozen with DATA by the SOURCE defect « application figée ».
    status_ids: Vec<u32>,
    main: Field,
    faults: Vec<(Category, Render, u64)>,
    supply: Option<String>,
    cp7: Vec<usize>,
}

/// The qualification of the truth file on the variant (`doc` the full pack JSON), drawn for one hidden test.
fn plan(doc: &Value, student: &Pack, q: &Value, rest: f64, draw: &mut Draw) -> Result<Plan, String> {
    let s = Schedule::parse(q)?;
    // the quantity follows one path through the five segments (the truth file gives them as one sequence)
    let path = serde_json::json!({"quantity": q["quantity"], "segments": [
        {"phase": "V-0", "ms": s.q0.1 - s.q0.0, "path": q["path"]["Q-0"]},
        {"phase": "V-A", "ms": s.qn.1 - s.qn.0, "path": q["path"]["Q-N"]},
        {"phase": "V-B", "ms": s.ql.1 - s.ql.0 + s.qs.1 - s.qs.0, "path": q["path"]["Q-LS"]},
        {"phase": "V-C", "ms": s.duration_ms - s.qs.1, "path": q["path"]["Q-F"]}]});
    let drawn = sequence::draw(&path, rest, draw)?;
    let main_source = q["main"].as_str().ok_or_else(|| bad("signal principal"))?;
    let (data_id, main) = data_field(doc, main_source)?;
    let data_ids = student.p22_data_ids(data_id).ok_or_else(|| bad("liste de Data ID"))?;
    let status = q["status"].as_str().ok_or_else(|| bad("statut"))?;
    let (status_id, status_field) = data_field(doc, status)?;
    if status_id != data_id {
        return Err(bad("statut hors de la trame DATA"));
    }
    let enumeration = status_field.enumeration.clone().ok_or_else(|| bad("statut sans liste"))?;
    let code = |label: &str| num(&doc["enums"][&enumeration][label]).ok_or_else(|| bad("valeur de statut"));
    let mut faults = Vec::new();
    let mut categories = vec![Category::Sensor, Category::Source, Category::Message];
    // the order of the three defects is drawn by the seed
    for i in (1..categories.len()).rev() {
        let j = (draw.next() % (i as u64 + 1)) as usize;
        categories.swap(i, j);
    }
    for (k, cat) in categories.into_iter().enumerate() {
        let key = match cat {
            Category::Sensor => "SENSOR",
            Category::Source => "SOURCE",
            Category::Message => "MESSAGE",
        };
        let list = q["faults"][key].as_array().filter(|l| !l.is_empty()).ok_or_else(|| bad("liste de défauts"))?;
        let item = &list[(draw.next() % list.len() as u64) as usize];
        let render = match item["render"].as_str() {
            Some("status_sna") => {
                if main.sna.is_none() {
                    return Err(bad("valeur sans code d'invalidité"));
                }
                Render::StatusSna { status: status_field.positions.clone(), code: code(item["label"].as_str().ok_or_else(|| bad("libellé"))?)? }
            }
            Some("frozen") => Render::Frozen,
            Some("out_of_range") => Render::OutOfRange,
            Some("corrupt") => Render::Corrupt,
            Some("wrong_data_ids") => Render::WrongDataIds,
            _ => return Err(bad("rendu de défaut")),
        };
        faults.push((cat, render, s.defects_at_ms[k]));
    }
    let cp7 = q["cp7_slots"].as_array().ok_or_else(|| bad("slots CP-7"))?.iter().map(|x| x.as_u64().map(|v| v as usize).ok_or_else(|| bad("slot"))).collect::<Result<_, _>>()?;
    let emitter = doc["source_ecu"]["emitter"].as_str().ok_or_else(|| bad("émetteur de la mission"))?;
    let status_ids = doc["frames"]
        .as_array()
        .ok_or_else(|| bad("trames"))?
        .iter()
        .filter(|f| f["emitter"].as_str() == Some(emitter) && f["protection"]["kind"].as_str() == Some("alive"))
        .map(|f| num(&f["id"]).map(|x| x as u32).ok_or_else(|| bad("identifiant")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Plan { s, drawn, data_id, data_ids, status_ids, main, faults, supply: q["supply"].as_str().map(str::to_owned), cp7 })
}

/// Renders the hidden qualification: the bench and the vehicle along the drawn path (the source ECU's supply cut for
/// the reset of Q-S), then the frame-level events.
fn render(student: &Pack, p: &Plan, vehicle_seed: u64, sensor_seed: u64) -> Result<Vec<BusFrame>, String> {
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let mut bench = VirtualBench::new(circuit, student, sensor_seed)?;
    let supply = match &p.supply {
        Some(d) => Some(bench.component(d).ok_or_else(|| bad("alimentation absente du banc"))?),
        None => None,
    };
    let quantity = p.drawn.quantity.clone();
    let mut rest = bench.sensor_state();
    rest.insert(quantity.clone(), p.drawn.at(0));
    bench.set_sensor(&rest)?;
    let mut vehicle = Vehicle::new(student, vehicle_seed);
    let mut last = None;
    let (reset, reset_end) = (p.s.supervision.reset_at_ms, p.s.supervision.reset_at_ms + p.s.supervision.reset_ms);
    let frames = bench.drive(&mut vehicle, p.s.duration_ms * 1000, &mut |ms, b| {
        if let Some(comp) = supply {
            if ms == reset || ms == reset_end {
                b.apply(&PhysCommand::Interact { interact: Interact::SetPower { comp, on: ms != reset } })?;
            }
        }
        let v = (p.drawn.at(ms) * 1e6).round() / 1e6;
        if last == Some(v.to_bits()) {
            return Ok(());
        }
        last = Some(v.to_bits());
        b.set_sensor(&[(quantity.clone(), v)].into_iter().collect::<BTreeMap<_, _>>())
    })?;
    Ok(reshape(frames, p))
}

fn counter(data: &[u8]) -> u8 {
    data.get(1).map_or(0, |b| b & 0x0F)
}

fn protect(data: &mut [u8], ids: &[u8; 16]) {
    if data.len() >= 2 {
        let c = counter(data);
        e2e::p22_protect(data, c, ids);
    }
}

/// The frame-level events of Q-N, Q-L, Q-S and Q-F on the rendered frames.
fn reshape(mut frames: Vec<BusFrame>, p: &Plan) -> Vec<BusFrame> {
    let ms = |f: &BusFrame| f.t_us / 1000;
    let is_data = |f: &BusFrame| f.id == p.data_id && f.flags == 0;
    // Q-L rewrites of the main value, protected again (counters kept)
    let raws = [p.main.max, (p.main.max + 1).min((1i64 << p.main.positions.len()) - 1), p.main.sna.unwrap_or(p.main.max)];
    for (k, w) in p.s.rewrites.iter().enumerate() {
        let raw = if k == 1 && Some(raws[1]) == p.main.sna { p.main.max } else { raws[k] };
        for f in frames.iter_mut().filter(|f| is_data(f) && in_window(ms(f), *w)) {
            write_raw(&mut f.data, &p.main.positions, raw);
            protect(&mut f.data, &p.data_ids);
        }
    }
    // Q-F defects
    for (_, render, start) in &p.faults {
        let w = (*start, start + p.s.defect_ms);
        if matches!(render, Render::Frozen) {
            // DC-M21: the application stops and COM sends again the last PDU of each of its frames, DATA and STATUS
            // together: content, E2E counter and alive counter identical
            for id in std::iter::once(p.data_id).chain(p.status_ids.iter().copied()) {
                let of = |f: &BusFrame| f.id == id && f.flags == 0;
                let held = frames
                    .iter()
                    .filter(|f| of(f) && ms(f) < w.0)
                    .last()
                    .or_else(|| frames.iter().find(|f| of(f) && in_window(ms(f), w)))
                    .map(|f| f.data.clone());
                if let Some(held) = held {
                    for f in frames.iter_mut().filter(|f| of(f) && in_window(ms(f), w)) {
                        f.data = held.clone();
                    }
                }
            }
            continue;
        }
        for f in frames.iter_mut().filter(|f| is_data(f) && in_window(ms(f), w)) {
            match render {
                Render::StatusSna { status, code } => {
                    write_raw(&mut f.data, status, *code);
                    write_raw(&mut f.data, &p.main.positions, p.main.sna.unwrap_or(p.main.max));
                    protect(&mut f.data, &p.data_ids);
                }
                Render::OutOfRange => {
                    let raw = (p.main.max + 1).min((1i64 << p.main.positions.len()) - 1);
                    write_raw(&mut f.data, &p.main.positions, if Some(raw) == p.main.sna { p.main.max } else { raw });
                    protect(&mut f.data, &p.data_ids);
                }
                Render::Frozen => {}
                Render::Corrupt => {
                    let lsb = p.main.positions[0];
                    if let Some(b) = f.data.get_mut(lsb as usize / 8) {
                        *b ^= 1 << (lsb % 8);
                    }
                }
                Render::WrongDataIds => {
                    let mut other = p.data_ids;
                    other.rotate_left(1);
                    protect(&mut f.data, &other);
                }
            }
        }
    }
    // Q-S: isolated wrong protection, repeated counter, delayed frame, burst, omissions (in this order: indices move)
    let data_at = |frames: &Vec<BusFrame>, t: u64| frames.iter().position(|f| is_data(f) && ms(f) >= t);
    let sv = &p.s.supervision;
    if let Some(i) = data_at(&frames, sv.wrong_at_ms) {
        frames[i].data[0] ^= 0x5A;
    }
    if let Some(i) = data_at(&frames, sv.repeat_at_ms) {
        if let Some(prev) = frames[..i].iter().rposition(|f| is_data(f)) {
            let copy = frames[prev].data.clone();
            frames[i].data = copy;
        }
    }
    if let Some(i) = data_at(&frames, sv.delay_at_ms) {
        frames[i].t_us += sv.delay_ms * 1000;
    }
    if let Some(i) = data_at(&frames, sv.burst_at_ms) {
        // two frames in one cycle: the next one comes right after this one
        if let Some(j) = frames[i + 1..].iter().position(|f| is_data(f)).map(|k| k + i + 1) {
            frames[j].t_us = frames[i].t_us + sv.burst_gap_ms * 1000;
        }
    }
    for (t, n) in [(sv.omit1_at_ms, 1usize), (sv.omit3_at_ms, 3)] {
        for _ in 0..n {
            if let Some(i) = data_at(&frames, t) {
                frames.remove(i);
            }
        }
    }
    // foreign frames, 0.3 ms after their millisecond
    let data_len = frames.iter().find(|f| is_data(f)).map_or(8, |f| f.data.len());
    let template = vec![0u8; data_len];
    for f in &p.s.foreign {
        let t_us = f.at_ms * 1000 + 300;
        let dlc = data_len as u8;
        frames.push(match f.kind {
            ForeignKind::Unknown => {
                let id = f.id.as_ref().and_then(num).unwrap_or(0) as u32;
                BusFrame { t_us, id, flags: 0, dlc, data: template.clone() }
            }
            ForeignKind::Ext => BusFrame { t_us, id: p.data_id, flags: n7rt_link::CAN_EXT, dlc, data: template.clone() },
            ForeignKind::Rtr => BusFrame { t_us, id: p.data_id, flags: n7rt_link::CAN_RTR, dlc, data: Vec::new() },
            ForeignKind::Fd => {
                let dlc = f.dlc.unwrap_or(9);
                BusFrame { t_us, id: p.data_id, flags: n7rt_link::CAN_FD, dlc, data: vec![0; fd_length(dlc)] }
            }
            ForeignKind::Short => {
                let dlc = f.dlc.unwrap_or(0);
                BusFrame { t_us, id: p.data_id, flags: 0, dlc, data: vec![0; dlc as usize] }
            }
        });
    }
    frames.sort_by_key(|f| (f.t_us, f.id, f.flags));
    frames
}

/// The competence of a point at `t_ms` on slot `slot` (`logic`: a logical slot): the first rule of the schedule whose
/// condition holds.
fn competence(p: &Plan, t_ms: u64, slot: usize, logic: bool) -> u8 {
    let s = &p.s;
    let wrong = s.supervision.wrong_at_ms;
    let mut protection = vec![(wrong, wrong + s.after_protection_ms)];
    let mut sensor = Vec::new();
    let mut source = Vec::new();
    let mut returns = Vec::new();
    for (cat, _, start) in &p.faults {
        let defect = (*start, start + s.defect_ms);
        match cat {
            Category::Message => protection.push((defect.0, defect.1 + s.after_protection_ms)),
            Category::Sensor => sensor.push(defect),
            Category::Source => source.push(defect),
        }
        returns.push((defect.1, start + 2 * s.defect_ms));
    }
    let foreign: Vec<(u64, u64)> = s.foreign.iter().map(|f| (f.at_ms, f.at_ms + s.after_foreign_ms + 1)).collect();
    let any = |ws: &[(u64, u64)]| ws.iter().any(|w| in_window(t_ms, *w));
    let holds = |when: When| match when {
        When::Protection => any(&protection),
        When::Start => in_window(t_ms, s.q0),
        When::Supervision => in_window(t_ms, s.qs),
        When::Sensor => any(&sensor),
        When::Returns => any(&returns),
        When::SafetyLogic => p.cp7.contains(&slot) && (in_window(t_ms, s.qn) || any(&source)),
        When::Limits => in_window(t_ms, s.ql),
        When::Foreign => any(&foreign),
        When::NominalValues => in_window(t_ms, s.qn) && !logic,
        When::Source => any(&source),
    };
    s.labels.iter().find(|r| holds(r.when)).map_or(s.otherwise, |r| r.cp)
}

/// Labels every written point of `expected` with its competence; refuses a competence with fewer than `min` points.
fn label(expected: &mut Expected, p: &Plan, min: usize) -> Result<(), String> {
    let mut counts = [0usize; COMPETENCES];
    let mut rows = Vec::with_capacity(expected.ticks.len());
    for (i, t) in expected.ticks.iter().enumerate() {
        let row: Vec<u8> = t
            .slots
            .iter()
            .enumerate()
            .map(|(s, v)| {
                if v.is_none() || i < expected.start {
                    return 0;
                }
                let logic = expected.rules.get(s).is_some_and(|r| r.logic);
                let cp = competence(p, t.t_us / 1000, s, logic);
                counts[cp as usize - 1] += 1;
                cp
            })
            .collect();
        rows.push(row);
    }
    if let Some(k) = counts.iter().position(|c| *c < min) {
        return Err(bad(&format!("CP-{} n'a que {} points", k + 1, counts[k])));
    }
    expected.cp = Some(rows);
    Ok(())
}

/// The pool of hidden qualification tests of a group (`doc`: the full variant JSON, `q`: the truth file's
/// qualification), each `{"duration_us", "vector", "expected"}` with competence labels.
pub fn tests(full: &Pack, student: &Pack, doc: &Value, q: &Value, pool: usize, seeds: &dyn Fn(usize, &str) -> u64) -> Result<Vec<Value>, String> {
    let matrix: Vec<u32> = full.frame_ids();
    let schedule = Schedule::parse(q)?;
    if schedule.unknown_ids().iter().any(|id| matrix.contains(id)) {
        return Err(bad("identifiant inconnu présent dans la matrice"));
    }
    let duration_us = schedule.duration_ms * 1000;
    let circuit = persist::load(student.bench_json()).map_err(|_| bad("banc de référence illisible"))?;
    let rest_state = VirtualBench::new(circuit, student, 0)?.sensor_state();
    let quantity = q["quantity"].as_str().ok_or_else(|| bad("grandeur"))?;
    let rest = *rest_state.get(quantity).ok_or_else(|| bad("grandeur absente du capteur"))?;
    let min = q["cp_min_points"].as_u64().unwrap_or(20) as usize;
    let mut out = Vec::with_capacity(pool);
    for i in 0..pool {
        let mut draw = Draw::new(seeds(i, "draw"));
        let p = plan(doc, student, q, rest, &mut draw)?;
        let frames = render(student, &p, seeds(i, "vehicle"), seeds(i, "scenario"))?;
        let vector = crate::workflow::vector::HiddenVector::new(duration_us, frames.into_iter().filter(|f| f.t_us <= duration_us).collect())?;
        let mut expected = crate::workflow::compile::expected(full, &vector)?;
        label(&mut expected, &p, min)?;
        out.push(serde_json::json!({"duration_us": vector.duration_us, "vector": vector.encode(), "expected": expected.to_json()}));
    }
    Ok(out)
}

/// Whether the bench of `pack` has a DC supply named `designator` (for the reset of Q-S).
pub fn has_supply(pack: &Pack, designator: &str) -> bool {
    persist::load(pack.bench_json()).ok().is_some_and(|c| c.components().values().any(|x| x.label == designator && x.kind == Kind::DcSupply))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frozen application (DC-M21) on synthetic frames with a fictitious schedule: in the defect window every DATA and
    /// every STATUS frame is the last one sent before it, byte for byte (counters included); outside, nothing changes.
    #[test]
    fn the_frozen_application_sends_its_last_data_and_status_unchanged() {
        let q = serde_json::json!({"schedule": {
            "duration_ms": 10000, "q0": [0, 500], "qn": [500, 3000], "ql": [3000, 5000], "qs": [5000, 7000],
            "defects_at_ms": [7000, 8000, 9000], "defect_ms": 300, "after_protection_ms": 100, "after_foreign_ms": 20,
            "rewrites": [[3100, 3200], [3300, 3400], [3500, 3600]], "foreign": [],
            "supervision": {"omit1_at_ms": 5100, "omit3_at_ms": 5300, "delay_at_ms": 5500, "delay_ms": 3, "burst_at_ms": 5700,
                            "burst_gap_ms": 1, "repeat_at_ms": 5900, "reset_at_ms": 6100, "reset_ms": 50, "wrong_at_ms": 6500},
            "labels": [{"when": "start", "cp": 5}], "otherwise": 2}});
        let s = Schedule::parse(&q).expect("valid fictitious schedule");
        let segment = |phase: &str| serde_json::json!({"phase": phase, "ms": 1000, "path": [[0, 0.0]]});
        let path = serde_json::json!({"quantity": "position", "segments": [segment("V-0"), segment("V-A"), segment("V-B"), segment("V-C")]});
        let drawn = sequence::draw(&path, 0.0, &mut Draw::new(1)).unwrap();
        let data_ids: [u8; 16] = std::array::from_fn(|i| 0x10 + i as u8);
        let p = Plan {
            s,
            drawn,
            data_id: 0x123,
            data_ids,
            status_ids: vec![0x456],
            main: Field { positions: (16..28).collect(), max: 4000, sna: Some(4095), enumeration: None },
            faults: vec![(Category::Source, Render::Frozen, 8000)],
            supply: None,
            cp7: Vec::new(),
        };
        let mut frames = Vec::new();
        for k in 0..1000u64 {
            let mut data = vec![0u8, (k % 16) as u8, (k % 251) as u8, (k / 251) as u8, 0, 0, 0, 0];
            protect(&mut data, &data_ids);
            frames.push(BusFrame { t_us: k * 10_000 + 1_000, id: 0x123, flags: 0, dlc: 8, data });
            frames.push(BusFrame { t_us: k * 10_000 + 2_000, id: 0x456, flags: 0, dlc: 4, data: vec![(k % 256) as u8, 7, 0, 0] });
        }
        let out = reshape(frames.clone(), &p);
        let window = |f: &BusFrame| (8_000_000..8_300_000).contains(&f.t_us);
        for id in [0x123u32, 0x456] {
            let last_before = frames.iter().filter(|f| f.id == id && f.t_us < 8_000_000).last().unwrap().data.clone();
            let inside: Vec<&BusFrame> = out.iter().filter(|f| f.id == id && window(f)).collect();
            assert!(!inside.is_empty() && inside.iter().all(|f| f.data == last_before), "{id:#x} frozen in the window");
            let after: Vec<&BusFrame> = out.iter().filter(|f| f.id == id && (8_400_000..8_500_000).contains(&f.t_us)).collect();
            assert!(after.iter().all(|f| frames.iter().any(|g| g.id == id && g.t_us == f.t_us && g.data == f.data)), "{id:#x} unchanged after");
        }
    }
}

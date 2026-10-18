//! Diagnostics of contract §3.7: computed states, never refusals. Topological rules read the computed netlist;
//! electrical rules read Valid readings only, so a decision never rests on an invalid reading (the diagnostic
//! then describes the invalid state). Stable order: severity, code, first item, message.

use std::collections::{BTreeMap, BTreeSet};

use n7rt_circuit::{kind_def, CompId, Component, ItemRef, Kind, Net, PinRef, Role};
use n7rt_solver::{Element, IslandStatus, NodeClass, NodeId, SupplyMode};

use super::lower::{ecu_can, ecu_powered, param_bool, param_int, param_num, state_bool, ECU_3V3_ILIM, ECU_5V_ILIM};
use super::probe::{Capture, Probe, RsIssue, MCU_VIH, MCU_VIL, TXD_VIH, TXD_VIL};
use super::util::{comp_label, pin_label, si};
use super::{Diagnostic, Reading, Severity, TxdState};

/// UM2407 Rev 4 p. 16 and p. 20: 300 mA available for shields on the 5V pin.
pub(crate) const RAIL_5V_OVERLOAD_A: f64 = 0.3;
/// DS12117 table 119: 20 mA sunk or sourced by an I/O.
pub(crate) const PD1_MAX_A: f64 = 0.02;
/// MCP2551 D31/D32: RXD specified at 8 mA.
pub(crate) const RXD_MAX_A: f64 = 0.008;
/// DS12117 table 118: FT pins within [−0.3 V ; VDD + 4.0 V].
pub(crate) const FT_MIN_V: f64 = -0.3;
pub(crate) const FT_ABOVE_VDD_V: f64 = 4.0;
/// MCP2551 absolute maximum VDD and operating range.
pub(crate) const XCVR_ABS_MAX_V: f64 = 7.0;
pub(crate) const XCVR_VDD_MIN_V: f64 = 4.5;
pub(crate) const XCVR_VDD_MAX_V: f64 = 5.5;
/// Polarized capacitor: reverse voltage flagged below −0.5 V (N7RT_MODEL).
pub(crate) const CAP_REVERSE_V: f64 = -0.5;

struct Out(Vec<Diagnostic>);

impl Out {
    fn push(&mut self, code: &str, severity: Severity, message_fr: String, items: Vec<ItemRef>) {
        self.0.push(Diagnostic { code: code.to_string(), severity, message_fr, items });
    }
}

fn comp_item(id: CompId) -> ItemRef {
    ItemRef::Component { id }
}

fn pin_item(p: &PinRef) -> ItemRef {
    ItemRef::Pin { comp: p.comp, pin: p.pin.clone() }
}

fn pin_ref(comp: CompId, pin: &str) -> PinRef {
    PinRef { comp, pin: pin.to_string() }
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn item_key(i: Option<&ItemRef>) -> (u8, u32, u8, String) {
    match i {
        None => (0, 0, 0, String::new()),
        Some(ItemRef::Component { id }) => (1, id.0, 0, String::new()),
        Some(ItemRef::Pin { comp, pin }) => (1, comp.0, 1, pin.clone()),
        Some(ItemRef::Wire { id }) => (2, id.0, 0, String::new()),
    }
}

/// Contract order: severity, then code, then first item (message as the last tie-break).
pub(crate) fn sort(diags: &mut [Diagnostic]) {
    diags.sort_by(|a, b| {
        (severity_rank(a.severity), &a.code, item_key(a.items.first()), &a.message_fr).cmp(&(
            severity_rank(b.severity),
            &b.code,
            item_key(b.items.first()),
            &b.message_fr,
        ))
    });
}

fn role(p: &Probe, pin: &PinRef) -> Option<Role> {
    p.comp(pin.comp).and_then(|c| kind_def(c.kind).pins.iter().find(|d| d.id == pin.pin)).map(|d| d.role)
}

/// A logic output that drives its net: RXD always, PD1 in `can` mode only.
fn active_logic_out(p: &Probe, pin: &PinRef) -> bool {
    match p.comp(pin.comp) {
        Some(c) if c.kind == Kind::EcuStm32 => pin.pin == "can_tx" && ecu_can(c),
        Some(c) => c.kind == Kind::CanTransceiver && pin.pin == "rxd",
        None => false,
    }
}

fn net_of<'n>(p: &'n Probe, pin: &PinRef) -> Option<&'n Net> {
    p.ctx.netlist.net_of(pin)
}

fn valid(r: &Reading<f64>) -> Option<f64> {
    match r {
        Reading::Valid { v } => Some(*v),
        _ => None,
    }
}

/// Every diagnostic of a frame, sorted.
pub(crate) fn compute(p: &Probe, cap: &Capture, notices: &[Diagnostic]) -> Vec<Diagnostic> {
    let mut out = Out(Vec::new());
    topological(p, &mut out);
    nets(p, &mut out);
    for c in p.ctx.circuit.components().values() {
        component(p, cap, c, &mut out);
    }
    islands(p, &mut out);
    if cap.load_error {
        out.push(
            "internal_error",
            Severity::Error,
            "Erreur interne : le réseau abaissé est refusé par le solveur, les résultats du réseau précédent sont conservés".to_string(),
            Vec::new(),
        );
    }
    out.0.extend(notices.iter().cloned());
    sort(&mut out.0);
    out.0
}

// ------------------------------------------------------------------------------------------------ topological

fn topological(p: &Probe, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    let comps = circuit.components();

    // no_ground: a source and no ground symbol.
    let has_ground = comps.values().any(|c| c.kind == Kind::Ground);
    let sources: Vec<ItemRef> = comps
        .values()
        .filter(|c| match c.kind {
            Kind::DcSupply | Kind::EcuStm32 => true,
            Kind::CanBus => param_int(c, "remote_nodes") >= 1,
            _ => false,
        })
        .map(|c| comp_item(c.id))
        .collect();
    if !has_ground && !sources.is_empty() {
        out.push("no_ground", Severity::Warning, "Aucune masse : tensions relatives seulement".to_string(), sources);
    }

    let alone = |pin: &PinRef| net_of(p, pin).map_or(true, |n| n.pins.len() <= 1);
    for c in comps.values() {
        let def = kind_def(c.kind);
        let wired = def.pins.iter().any(|d| !alone(&pin_ref(c.id, d.id)));
        for d in def.pins {
            let pin = pin_ref(c.id, d.id);
            if !alone(&pin) {
                continue;
            }
            // unconnected_pin: a required pin alone while the component is wired elsewhere.
            let required = d.required && (c.kind != Kind::CanBus || param_int(c, "remote_nodes") >= 1);
            if required && wired {
                out.push(
                    "unconnected_pin",
                    Severity::Warning,
                    format!("Broche {} non reliée alors que {} est câblé", pin_label(circuit, &pin), comp_label(circuit, c.id)),
                    vec![pin_item(&pin)],
                );
            }
            // open_pin: a passive pin alone, no current through it.
            if d.role == Role::Passive && c.kind != Kind::Junction {
                out.push(
                    "open_pin",
                    Severity::Info,
                    format!("{} non reliée : circuit ouvert, aucun courant ne la traverse", pin_label(circuit, &pin)),
                    vec![pin_item(&pin)],
                );
            }
        }
        // source_short: both terminals of a supply on one net.
        if c.kind == Kind::DcSupply {
            let (pos, neg) = (pin_ref(c.id, "pos"), pin_ref(c.id, "neg"));
            if let (Some(a), Some(b)) = (net_of(p, &pos), net_of(p, &neg)) {
                if a.key == b.key {
                    out.push(
                        "source_short",
                        Severity::Error,
                        format!("{} : bornes + et \u{2212} sur le même net, sortie en court-circuit", comp_label(circuit, c.id)),
                        vec![comp_item(c.id)],
                    );
                }
            }
        }
    }

    // output_short: a driving logic output with another one, or with a source or ground pin.
    for net in &p.ctx.netlist.nets {
        let outs: Vec<&PinRef> = net.pins.iter().filter(|q| active_logic_out(p, q)).collect();
        if outs.is_empty() {
            continue;
        }
        let others: Vec<&PinRef> = net
            .pins
            .iter()
            .filter(|q| !active_logic_out(p, q) && matches!(role(p, q), Some(Role::Source | Role::Ground | Role::GroundPin)))
            .collect();
        if outs.len() < 2 && others.is_empty() {
            continue;
        }
        let first = outs[0];
        let mut culprits: Vec<&PinRef> = outs[1..].to_vec();
        culprits.extend(others);
        let mut items: Vec<ItemRef> = outs.iter().map(|q| pin_item(q)).collect();
        items.extend(culprits.iter().filter(|q| !outs.contains(q)).map(|q| pin_item(q)));
        let names: Vec<String> = culprits.iter().map(|q| pin_label(circuit, q)).collect();
        out.push(
            "output_short",
            Severity::Error,
            format!("Court-circuit logique sur le net {} : la sortie {} est reliée à {}", net.name, pin_label(circuit, first), names.join(", ")),
            items,
        );
    }
}

// ------------------------------------------------------------------------------------------------------- nets

fn nets(p: &Probe, out: &mut Out) {
    // floating_net: Floating class with ≥ 2 pins, or with an input.
    for net in &p.ctx.netlist.nets {
        let Some(n) = p.ctx.low.node_of.get(&net.key).copied() else { continue };
        if net.ground || p.blocked.contains_key(&n) || p.node_reading(n) != Reading::Floating {
            continue;
        }
        let input = net.pins.iter().any(|q| matches!(role(p, q), Some(Role::AnalogIn | Role::LogicIn | Role::PowerIn)));
        if net.pins.len() >= 2 || input {
            out.push(
                "floating_net",
                Severity::Warning,
                format!("Net {} flottant : potentiel indéterminé, aucun chemin vers une référence", net.name),
                net.pins.iter().map(pin_item).collect(),
            );
        }
    }

    // island_no_reference: a relative K group (no ground) that contains a source.
    let mut groups: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
    for (n, r) in p.res.nodes.iter().enumerate() {
        if r.class == NodeClass::Relative && p.island_issue(n).is_none() {
            if let Some(k) = r.k {
                groups.entry(k).or_default().push(n);
            }
        }
    }
    let mut with_source = BTreeSet::new();
    for e in &p.ctx.low.network.elements {
        let (pos, neg) = match e {
            Element::Supply { pos, neg, .. } | Element::VSource { pos, neg, .. } => (*pos, *neg),
            _ => continue,
        };
        for node in [pos, neg] {
            if let Some(k) = p.res.nodes.get(node).filter(|r| r.class == NodeClass::Relative).and_then(|r| r.k) {
                with_source.insert(k);
            }
        }
    }
    for (k, nodes) in &groups {
        if !with_source.contains(k) {
            continue;
        }
        let nets: Vec<&Net> = nodes.iter().filter_map(|n| p.ctx.netlist.nets.get(*n)).collect();
        if nets.is_empty() {
            continue;
        }
        let names: Vec<&str> = nets.iter().map(|n| n.name.as_str()).collect();
        out.push(
            "island_no_reference",
            Severity::Warning,
            format!("Îlot sans masse ({}) : tensions relatives seulement", names.join(", ")),
            nets.iter().map(|n| pin_item(&n.key)).collect(),
        );
    }
}

// -------------------------------------------------------------------------------------------------- components

fn component(p: &Probe, cap: &Capture, c: &Component, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    let id = c.id;
    let label = comp_label(circuit, id);
    let low = p.ctx.low.comps.get(&id);
    let elem_current = |name: &str| low.and_then(|l| l.elem(name)).and_then(|e| p.elem_current(e).ok());
    match c.kind {
        Kind::DcSupply => {
            if state_bool(circuit, c, "on") {
                if let Some(e) = low.and_then(|l| l.elem("supply")) {
                    supply_mode_diag(p, e, &label, param_num(c, "current_limit"), param_num(c, "voltage"), id, out);
                }
            }
        }
        Kind::Resistor | Kind::Termination => {
            if let Some(u) = valid(&p.pins_between(id, "p1", "p2")) {
                let r = param_num(c, "resistance");
                let rating = param_num(c, "power_rating");
                let pw = u * u / r;
                if pw > rating {
                    out.push(
                        "resistor_overload",
                        Severity::Error,
                        format!("{label} : {} dissipés pour {} nominaux", si(pw, "W"), si(rating, "W")),
                        vec![comp_item(id)],
                    );
                }
            }
        }
        Kind::Potentiometer => {
            let r = param_num(c, "resistance");
            let rating = param_num(c, "power_rating");
            let limit = (rating / r).sqrt();
            let worst = ["r_aw", "r_wb"].iter().filter_map(|n| elem_current(n)).map(f64::abs).fold(None, |m: Option<f64>, i| {
                Some(m.map_or(i, |m| m.max(i)))
            });
            if let Some(i) = worst.filter(|i| *i > limit) {
                out.push(
                    "resistor_overload",
                    Severity::Error,
                    format!("{label} : {} dans un segment, au-delà de {} (\u{221a}(P/R))", si(i, "A"), si(limit, "A")),
                    vec![comp_item(id)],
                );
            }
        }
        Kind::Capacitor => {
            if let Some(u) = valid(&p.pins_between(id, "p1", "p2")) {
                let rating = param_num(c, "voltage_rating");
                if u.abs() > rating {
                    out.push(
                        "cap_overvoltage",
                        Severity::Error,
                        format!("{label} : {} aux bornes, au-delà de sa tension nominale de {}", si(u, "V"), si(rating, "V")),
                        vec![comp_item(id)],
                    );
                }
                if param_bool(c, "polarized") && u < CAP_REVERSE_V {
                    out.push(
                        "cap_reverse",
                        Severity::Error,
                        format!("{label} polarisé en inverse : {} entre + et \u{2212}", si(u, "V")),
                        vec![comp_item(id)],
                    );
                }
            }
        }
        Kind::AdcInput => {
            adc_input_diag(p, id, "in", "ref", &label, out);
            adc_sample_diag(cap, circuit, id, "in", out);
        }
        Kind::EcuStm32 => ecu_diag(p, cap, c, &label, out),
        Kind::CanTransceiver => xcvr_diag(p, cap, c, &label, out),
        Kind::EcuApps => ecu_apps_diag(p, id, &label, out),
        Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps => {
            if p.part_gate(id,None)==Some(false) {out.push("ic_unpowered",Severity::Warning,
                format!("{label} en reset : alimentation capteur coupée"),vec![comp_item(id)]);}
            for name in ["rail_l","rail_r","rail_5v"] {
                if let Some(e)=low.and_then(|l|l.elem(name)) {
                    if p.part_gate(id,None)==Some(true)&&p.res.modes.get(e.index).copied().flatten()==Some(SupplyMode::Cc) {
                        out.push("supply_current_limit",Severity::Warning,format!("{label} : sortie capteur en limitation de courant"),vec![comp_item(id)]);
                    }
                }
            }
            if c.kind==Kind::EcuWsf {
                for pin in ["ws_l_in","ws_r_in"] {
                    if let Some(v)=valid(&p.pins_between(id,pin,"gnd")) {
                        let power=v*v/param_num(c,"r_sense");
                        if power>param_num(c,"r_sense_rating") {out.push("resistor_overload",Severity::Error,
                            format!("{label} : résistance de mesure surchargée"),vec![comp_item(id)]);}
                    }
                }
            }
        }
        Kind::Ats682Lsh => {
            let model=super::wsf::SensorParams::from_component(c);
            if let Some(v)=valid(&p.pins_between(id,"vcc","gnd")) {
                if v<model.vdd_min||v>model.vdd_max {out.push("vdd_out_of_spec",Severity::Warning,
                    format!("{label} : alimentation hors de la plage de fonctionnement"),vec![comp_item(id)]);}
            }
        }
        Kind::Tle5014 | Kind::Pss260 => {
            let supply=if c.kind==Kind::Tle5014 {"vdd"}else{"us"};
            if let Some(v)=valid(&p.pins_between(id,supply,"gnd")) {
                let max=param_num(c,"abs_max_v");
                if v>max {out.push("ic_abs_max",Severity::Error,format!("{label} : surtension d'alimentation"),vec![comp_item(id)]);}
                if c.kind==Kind::Tle5014 && v>param_num(c,"overvoltage_v") {
                    out.push("ic_overvoltage",Severity::Warning,format!("{label} : alimentation hors plage fonctionnelle, sortie en haute impédance"),vec![comp_item(id)]);
                }
            }
            if let Some(e)=low.and_then(|l|l.elem("out_drv")) {
                if matches!(p.res.modes.get(e.index).copied().flatten(),Some(SupplyMode::Cc|SupplyMode::CcNeg)) {
                    out.push("output_overcurrent",Severity::Warning,format!("{label} : sortie en limitation de courant"),vec![comp_item(id)]);
                }
            }
        }
        Kind::Mlx90377 => {
            for die in 0..2 {
                if p.part_gate(id, Some(die)) == Some(false) {
                    out.push(
                        "ic_unpowered",
                        Severity::Warning,
                        format!("{label} : puce {} sous son seuil de démarrage, sortie en haute impédance", die + 1),
                        vec![comp_item(id)],
                    );
                }
            }
        }
        _ => {}
    }
}

/// A black box: its state as a part (in reset) and its sensor rails seen from the connector. No message quotes a
/// value of its hidden parameters.
fn ecu_apps_diag(p: &Probe, id: CompId, label: &str, out: &mut Out) {
    match p.part_gate(id, None) {
        Some(false) => out.push(
            "ic_unpowered",
            Severity::Warning,
            format!("{label} en reset : LV_IN \u{2212} GND sous le seuil de sortie de reset, rails capteur coupés"),
            vec![comp_item(id)],
        ),
        Some(true) => {
            let low = p.ctx.low.comps.get(&id);
            for (rail, name) in [("rail_1", "5V_1"), ("rail_2", "5V_2")] {
                let Some(e) = low.and_then(|l| l.elem(rail)) else { continue };
                match p.res.modes.get(e.index).copied().flatten() {
                    Some(SupplyMode::Cc) => out.push(
                        "supply_current_limit",
                        Severity::Warning,
                        format!("{label} : rail {name} en limitation de courant"),
                        vec![comp_item(id)],
                    ),
                    Some(SupplyMode::Rev) => out.push(
                        "supply_backfeed",
                        Severity::Warning,
                        format!("{label} : rail {name} poussé au-dessus de sa consigne, il ne débite plus"),
                        vec![comp_item(id)],
                    ),
                    _ => {}
                }
            }
        }
        None => {}
    }
}

fn supply_mode_diag(p: &Probe, e: &super::lower::ElemInfo, what: &str, i_lim: f64, v_set: f64, id: CompId, out: &mut Out) {
    match p.res.modes.get(e.index).copied().flatten() {
        Some(SupplyMode::Cc) => out.push(
            "supply_current_limit",
            Severity::Warning,
            format!("{what} en limitation de courant ({})", si(i_lim, "A")),
            vec![comp_item(id)],
        ),
        Some(SupplyMode::Rev) => out.push(
            "supply_backfeed",
            Severity::Warning,
            format!("{what} poussée au-dessus de sa consigne de {} : elle ne débite plus", si(v_set, "V")),
            vec![comp_item(id)],
        ),
        _ => {}
    }
}

fn adc_input_diag(p: &Probe, id: CompId, ch: &str, gnd: &str, label: &str, out: &mut Out) {
    let pin = pin_ref(id, ch);
    let short = pin_label(&p.ctx.circuit, &pin);
    match p.pins_between(id, ch, gnd) {
        Reading::Floating => out.push(
            "adc_floating",
            Severity::Warning,
            format!("{label} : entrée {short} flottante, échantillon indéterminé"),
            vec![pin_item(&pin)],
        ),
        Reading::NoReference => out.push(
            "adc_floating",
            Severity::Warning,
            format!("{label} : entrée {short} sans référence commune avec sa masse locale"),
            vec![pin_item(&pin)],
        ),
        _ => {}
    }
}

fn adc_sample_diag(cap: &Capture, circuit: &n7rt_circuit::Circuit, id: CompId, ch: &'static str, out: &mut Out) {
    if let Some(Reading::Saturated { v: s }) = cap.adc.get(&(id, ch)) {
        let pin = pin_ref(id, ch);
        out.push(
            "adc_saturated",
            Severity::Warning,
            format!(
                "{} : échantillon de {} hors de [0 ; {}[, code saturé à {}",
                pin_label(circuit, &pin),
                si(s.volts, "V"),
                si(s.vref, "V"),
                s.code
            ),
            vec![pin_item(&pin)],
        );
    }
}

fn logic_diag(p: &Probe, pin: &PinRef, vil: f64, vih: f64, against: &str, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    match p.logic(pin) {
        Reading::Floating => out.push(
            "logic_floating",
            Severity::Warning,
            format!("{} : entrée logique flottante, niveau indéterminé", pin_label(circuit, pin)),
            vec![pin_item(pin)],
        ),
        Reading::NoReference => out.push(
            "logic_floating",
            Severity::Warning,
            format!("{} : entrée logique sans référence commune avec {against}", pin_label(circuit, pin)),
            vec![pin_item(pin)],
        ),
        Reading::Undefined => {
            let v = valid(&p.pins_between(pin.comp, &pin.pin, against)).unwrap_or(f64::NAN);
            out.push(
                "logic_undefined",
                Severity::Warning,
                format!(
                    "{} : niveau logique indéfini ({} entre VIL = {} et VIH = {})",
                    pin_label(circuit, pin),
                    si(v, "V"),
                    si(vil, "V"),
                    si(vih, "V")
                ),
                vec![pin_item(pin)],
            );
        }
        _ => {}
    }
}

fn ecu_diag(p: &Probe, cap: &Capture, c: &Component, label: &str, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    let id = c.id;
    let powered = ecu_powered(circuit, c);
    if !powered {
        out.push(
            "ic_unpowered",
            Severity::Warning,
            format!("{label} : carte non alimentée (USB débranché)"),
            vec![comp_item(id)],
        );
    }
    let low = p.ctx.low.comps.get(&id);
    if powered {
        for (rail, name, ilim, v) in [("rail_5v", "5V", ECU_5V_ILIM, 5.0), ("rail_3v3", "3V3", ECU_3V3_ILIM, 3.3)] {
            if let Some(e) = low.and_then(|l| l.elem(rail)) {
                supply_mode_diag(p, e, &format!("{label} : rail {name}"), ilim, v, id, out);
            }
        }
        if let Some(i) = low.and_then(|l| l.elem("rail_5v")).and_then(|e| p.elem_current(e).ok()) {
            if i > RAIL_5V_OVERLOAD_A {
                out.push(
                    "rail_overload",
                    Severity::Warning,
                    format!("{label} : rail 5V à {}, au-delà des 300 mA disponibles (UM2407)", si(i, "A")),
                    vec![comp_item(id)],
                );
            }
        }
        for ch in ["a0", "a1"] {
            adc_input_diag(p, id, ch, "gnd", label, out);
            adc_sample_diag(cap, circuit, id, ch, out);
        }
        let vdd = valid(&p.pins_between(id, "v3v3", "gnd"));
        if ecu_can(c) {
            if let Some(vdd) = vdd {
                logic_diag(p, &pin_ref(id, "can_rx"), MCU_VIL * vdd, MCU_VIH * vdd, "gnd", out);
            }
            if let Some(i) = valid(&p.pin_current(&pin_ref(id, "can_tx"))) {
                if i.abs() > PD1_MAX_A {
                    out.push(
                        "output_overcurrent",
                        Severity::Error,
                        format!("{} : {} débités, au-delà de 20 mA (DS12117 table 119)", pin_label(circuit, &pin_ref(id, "can_tx")), si(i.abs(), "A")),
                        vec![pin_item(&pin_ref(id, "can_tx"))],
                    );
                }
            }
        }
    }
    // FT pin limits, powered or not: VDD = V(3V3) when powered, 0 otherwise.
    let vdd_ecu = if powered { valid(&p.pins_between(id, "v3v3", "gnd")) } else { Some(0.0) };
    if let Some(vdd) = vdd_ecu {
        let max = vdd + FT_ABOVE_VDD_V;
        for pin in ["can_rx", "can_tx"] {
            if let Some(v) = valid(&p.pins_between(id, pin, "gnd")) {
                if v < FT_MIN_V || v > max {
                    let pr = pin_ref(id, pin);
                    out.push(
                        "ft_pin_abs_max",
                        Severity::Error,
                        format!(
                            "{} à {} hors de [{} ; {}] (broche FT, DS12117 table 118)",
                            pin_label(circuit, &pr),
                            si(v, "V"),
                            si(FT_MIN_V, "V"),
                            si(max, "V")
                        ),
                        vec![pin_item(&pr)],
                    );
                }
            }
        }
    }
}

fn xcvr_diag(p: &Probe, cap: &Capture, c: &Component, label: &str, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    let id = c.id;
    let Some(x) = cap.xcvr.get(&id) else { return };
    let u = valid(&p.pins_between(id, "vdd", "vss"));
    if let Some(u) = u {
        if u > XCVR_ABS_MAX_V {
            out.push(
                "ic_abs_max",
                Severity::Error,
                format!("{label} : VDD \u{2212} VSS = {}, au-delà du maximum absolu de 7 V", si(u, "V")),
                vec![comp_item(id)],
            );
        }
    }
    if x.gate_known && !x.powered {
        out.push(
            "ic_unpowered",
            Severity::Warning,
            format!("{label} non alimenté : VDD \u{2212} VSS sous le seuil de mise sous tension (POR)"),
            vec![comp_item(id)],
        );
    }
    if !x.powered {
        return;
    }
    if let Some(u) = u {
        if !(XCVR_VDD_MIN_V..=XCVR_VDD_MAX_V).contains(&u) {
            out.push(
                "vdd_out_of_spec",
                Severity::Warning,
                format!("{label} : VDD \u{2212} VSS = {}, hors de la plage de fonctionnement [4.5 V ; 5.5 V]", si(u, "V")),
                vec![comp_item(id)],
            );
        }
    }
    let rs = pin_ref(id, "rs");
    match &x.rs_issue {
        Some(RsIssue::Floating) => out.push(
            "rs_floating",
            Severity::Warning,
            format!("{} flottante : mode du transceiver indéfini", pin_label(circuit, &rs)),
            vec![pin_item(&rs)],
        ),
        Some(RsIssue::NotModelled(why)) => out.push(
            "rs_non_modelise",
            Severity::Warning,
            format!("{} : mode non modélisé en V1 ({why})", pin_label(circuit, &rs)),
            vec![pin_item(&rs)],
        ),
        None => {}
    }
    if x.standby {
        out.push(
            "ic_standby",
            Severity::Info,
            format!("{label} en veille : Rs au-dessus de 0.75·VDD, émetteur coupé"),
            vec![comp_item(id)],
        );
    }
    logic_diag(p, &pin_ref(id, "txd"), TXD_VIL, TXD_VIH, "vss", out);
    match x.txd {
        TxdState::DominantRequested => out.push(
            "dominant_requested",
            Severity::Warning,
            format!(
                "{label} : TXD n'est pas au niveau haut, dominant demandé ; non modélisé en V1, lectures de CANH, CANL et RXD indisponibles"
            ),
            vec![comp_item(id)],
        ),
        TxdState::DominantTimeout => out.push(
            "dominant_timeout",
            Severity::Warning,
            format!("{label} : TXD bas depuis plus de 1.25 ms, émetteur coupé par la minuterie de dominant permanent ; bus récessif"),
            vec![comp_item(id)],
        ),
        TxdState::Recessive => {}
    }
    if let Some(i) = valid(&p.pin_current(&pin_ref(id, "rxd"))) {
        if i.abs() > RXD_MAX_A {
            let pr = pin_ref(id, "rxd");
            // §2.1: a warning (outside D31/D32, not an absolute maximum), unlike PD1.
            out.push(
                "output_overcurrent",
                Severity::Warning,
                format!("{} : {} débités, au-delà des 8 mA spécifiés (D31/D32)", pin_label(circuit, &pr), si(i.abs(), "A")),
                vec![pin_item(&pr)],
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------- islands

fn islands(p: &Probe, out: &mut Out) {
    let circuit = &p.ctx.circuit;
    for (i, status) in p.res.islands.iter().enumerate() {
        let (code, why) = match status {
            IslandStatus::Singular => ("solver_singular", "matrice singulière"),
            IslandStatus::NonFinite => ("solver_singular", "valeur non finie"),
            IslandStatus::ModeCycle => ("solver_mode_cycle", "modes CV/CC ou portes sans état stable"),
            IslandStatus::Solved { .. } | IslandStatus::Pending => continue,
        };
        let mut comps = BTreeSet::new();
        for (id, low) in &p.ctx.low.comps {
            let inside = low.elems.iter().any(|e| e.terms.iter().any(|t| p.res.nodes.get(t.node).and_then(|r| r.island) == Some(i)));
            if inside {
                comps.insert(*id);
            }
        }
        let names: Vec<String> = comps.iter().map(|id| comp_label(circuit, *id)).collect();
        out.push(
            code,
            Severity::Error,
            format!("Îlot non résolu ({why}) : {} ; aucune valeur publiée", names.join(", ")),
            comps.into_iter().map(comp_item).collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    //! Mapping of the solver island statuses, which no valid circuit of the library produces (every source has a
    //! series resistance): synthetic results on a lowered network.

    use std::sync::Arc;

    use super::*;
    use crate::phys::lower::Lowered;
    use crate::phys::probe::{Ctx, NodeRes, Results};
    use n7rt_circuit::{Circuit, Netlist};
    use n7rt_solver::{Network, SolverStats};

    fn capture(status: IslandStatus, load_error: bool) -> Capture {
        let low = Lowered {
            network: Network {
                nodes: 1,
                ground: None,
                elements: vec![Element::Resistor { a: 0, b: 0, r: 1.0, leakage: false, gate: None }],
                gates: vec![],
            },
            netlist: Netlist::default(),
            node_of: BTreeMap::new(),
            comps: BTreeMap::new(),
        };
        let ctx = Ctx { circuit: Circuit::new("t"), netlist: Netlist::default(), low: Arc::new(low), rev: 0 };
        let res = Results {
            time_ns: 0,
            preview: false,
            nodes: vec![NodeRes { volts: 0.0, class: NodeClass::Unused, k: None, island: Some(0) }],
            islands: vec![status],
            currents: vec![None],
            modes: vec![None],
            gates: vec![],
            stats: SolverStats::default(),
        };
        Capture { ctx: Arc::new(ctx), res: Arc::new(res), load_error, ..Capture::empty() }
    }

    fn codes(cap: &Capture) -> Vec<String> {
        let p = Probe { ctx: &cap.ctx, res: &cap.res, blocked: &cap.blocked };
        compute(&p, cap, &[]).into_iter().map(|d| d.code).collect()
    }

    #[test]
    fn solver_singular_pair() {
        assert_eq!(codes(&capture(IslandStatus::Singular, false)), vec!["solver_singular"]);
        assert_eq!(codes(&capture(IslandStatus::NonFinite, false)), vec!["solver_singular"]);
        assert!(codes(&capture(IslandStatus::Solved { ill_conditioned: true }, false)).is_empty());
    }

    #[test]
    fn solver_mode_cycle_pair() {
        assert_eq!(codes(&capture(IslandStatus::ModeCycle, false)), vec!["solver_mode_cycle"]);
        assert!(codes(&capture(IslandStatus::Solved { ill_conditioned: false }, false)).is_empty());
    }

    #[test]
    fn internal_error_pair() {
        assert_eq!(codes(&capture(IslandStatus::Solved { ill_conditioned: false }, true)), vec!["internal_error"]);
        assert!(codes(&capture(IslandStatus::Solved { ill_conditioned: false }, false)).is_empty());
    }

    #[test]
    fn unsolved_island_readings_are_unavailable() {
        let cap = capture(IslandStatus::ModeCycle, false);
        let p = Probe { ctx: &cap.ctx, res: &cap.res, blocked: &cap.blocked };
        assert_eq!(p.node_reading(0), Reading::Unavailable { reason: "mode_unresolved".into() });
        let cap = capture(IslandStatus::Singular, false);
        let p = Probe { ctx: &cap.ctx, res: &cap.res, blocked: &cap.blocked };
        assert_eq!(p.node_between(0, 0), Reading::Unavailable { reason: "solver_singular".into() });
    }

    #[test]
    fn order_is_severity_code_item() {
        let d = |code: &str, severity, items: Vec<ItemRef>| Diagnostic { code: code.into(), severity, message_fr: String::new(), items };
        let mut v = vec![
            d("open_pin", Severity::Info, vec![comp_item(CompId(1))]),
            d("floating_net", Severity::Warning, vec![comp_item(CompId(9))]),
            d("floating_net", Severity::Warning, vec![comp_item(CompId(2))]),
            d("source_short", Severity::Error, vec![]),
            d("adc_floating", Severity::Warning, vec![]),
        ];
        sort(&mut v);
        let got: Vec<(&str, Option<ItemRef>)> = v.iter().map(|x| (x.code.as_str(), x.items.first().cloned())).collect();
        assert_eq!(
            got,
            vec![
                ("source_short", None),
                ("adc_floating", None),
                ("floating_net", Some(comp_item(CompId(2)))),
                ("floating_net", Some(comp_item(CompId(9)))),
                ("open_pin", Some(comp_item(CompId(1)))),
            ]
        );
    }
}

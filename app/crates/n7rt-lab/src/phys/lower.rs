//! Lowering of a circuit into the solver network (contract docs/physical-lab-v1.md §2.1 and §8.1).
//!
//! Nets become nodes in NetKey order, component-internal nodes follow by (CompId, local index), elements are
//! emitted by (CompId, local index), and every state key hashes (CompId, element name): a capacitor keeps its
//! voltage across edits that renumber the nodes, and the same circuit always gives the same network.

use std::collections::{BTreeMap, BTreeSet};

use n7rt_circuit::{kind_def, Circuit, CompId, Component, Kind, Lit, Netlist, PinRef, Value, WireId};
use n7rt_solver::{Element, Gate, GateId, Network, NodeId, StateKey};

use super::defects::WireDefect;
use super::util::Fnv;

// Model values of contract §2.1 (provenance there and in the library sources_fr).
/// ECU 5V rail: UM2407 Rev 4 p. 20, current-limited switch at 500 mA.
pub(crate) const ECU_5V: f64 = 5.0;
pub(crate) const ECU_5V_ILIM: f64 = 0.5;
/// ECU 3V3 rail: ST1L05 (DocID14492 Rev 3, table 8), limit ≥ 1.3 A.
pub(crate) const ECU_3V3: f64 = 3.3;
pub(crate) const ECU_3V3_ILIM: f64 = 1.3;
/// PD1 driver: DS12117 table 157, VOH ≥ VDD − 0.4 V at 8 mA.
pub(crate) const ECU_PD1_R_OUT: f64 = 50.0;
/// Fraction of a SENT frame the line spends high (TLE5014 Final Datasheet rev. 1.0 p. 33: with the pause pulse the
/// frame lasts 282 UT; its 10 pulses (synchronisation, 8 nibbles, pause) are 5 UT low each, p. 33): the level a
/// voltmeter averages on an active line, 232/282 ≈ 0.823 of the driven high level.
pub(crate) const SENT_HIGH_FRACTION: f64 = 232.0 / 282.0;
/// MCP2551 (DS20001667G): TXD pull-up, IDD as a resistance, POR thresholds, driver output resistances.
pub(crate) const XCVR_TXD_PULLUP: f64 = 25e3;
pub(crate) const XCVR_R_DD: f64 = 500.0;
pub(crate) const XCVR_POR_ON: f64 = 4.05;
pub(crate) const XCVR_POR_OFF: f64 = 3.70;
pub(crate) const XCVR_BIAS_R_OUT: f64 = 1e-3;
pub(crate) const XCVR_RXD_R_OUT: f64 = 187.5;
pub(crate) const XCVR_VREF_R_OUT: f64 = 5e3;
/// Rest of the bus: remote termination (ISO 11898-2) and VDD/2 bias of the remote transceivers.
pub(crate) const BUS_TERMINATION: f64 = 120.0;
pub(crate) const BUS_BIAS_V: f64 = 2.5;
pub(crate) const BUS_BIAS_R_OUT: f64 = 1.0;
/// Numerical floor of a potentiometer segment.
pub(crate) const POT_FLOOR: f64 = 1e-3;
/// ECU APPS (black box, docs/apps-mission-slice.md §5): every conditioning value is a hidden parameter of the
/// part; only these generic constants are code. GND_S1 / GND_S2 tied to GND inside (numerical tie).
pub(crate) const ECU_APPS_GND_TIE: f64 = 1e-3;
/// Bus side of its MCP2551-type transceiver: R_IN of the can_transceiver model, bias at VDD/2 of a 5.0 V internal
/// rail, present only out of reset.
pub(crate) const ECU_APPS_XCVR_R_IN: f64 = 25e3;
pub(crate) const ECU_APPS_BIAS_V: f64 = 2.5;
/// ADC of the ECU: law of `adc_input` at 12 bits, fixed 3.3 V reference, sampled every 1 ms frame.
pub(crate) const ECU_APPS_ADC_BITS: u32 = 12;
pub(crate) const ECU_APPS_ADC_VREF: f64 = 3.3;
/// ADC channels of the ECU APPS: midpoints of the SIG dividers, then of the rail measurement dividers.
pub(crate) const ECU_APPS_CHANNELS: [&str; 4] = ["sig1", "sig2", "sup1", "sup2"];
/// Smallest hysteresis of a gate read from hidden thresholds (guard against an inverted pair in a bench file).
const GATE_MIN_HYSTERESIS: f64 = 1e-3;

/// One terminal of an element, as seen from its component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Term {
    /// Library pin id, or None for a component-internal node (`bias`, `rbias`).
    pub pin: Option<&'static str>,
    pub node: NodeId,
    /// +1 when the solver current of the element (its reference direction) enters the component here.
    pub sign: f64,
}

/// An element of the network and the component terminals it is attached to (control inputs of a Vcvs draw no
/// current and are not terminals).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ElemInfo {
    /// Index in [`Network::elements`].
    pub index: usize,
    pub name: &'static str,
    pub terms: [Term; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CompLow {
    pub elems: Vec<ElemInfo>,
    /// Power-on reset gate of a transceiver, power (reset) gate of an ECU APPS.
    pub gate: Option<GateId>,
    /// Start-up gates of the dies of a position sensor, die 1 first.
    pub dies: Vec<GateId>,
    pub internal: Vec<NodeId>,
    /// Internal nodes read by name (ADC channels of an ECU APPS).
    pub named: Vec<(&'static str, NodeId)>,
}

impl CompLow {
    pub fn elem(&self, name: &str) -> Option<&ElemInfo> {
        self.elems.iter().find(|e| e.name == name)
    }

    pub fn node(&self, name: &str) -> Option<NodeId> {
        self.named.iter().find(|(n, _)| *n == name).map(|(_, id)| *id)
    }
}

/// A circuit lowered to the solver network, with the maps that read results back by pin and component.
#[derive(Clone, Debug, Default)]
pub(crate) struct Lowered {
    pub network: Network,
    /// The nets that were numbered: node i is `netlist.nets[i]`.
    pub netlist: Netlist,
    /// Node of every pin of every component.
    pub node_of: BTreeMap<PinRef, NodeId>,
    pub comps: BTreeMap<CompId, CompLow>,
}

/// Stable identity of a stateful element across reloads.
pub(crate) fn state_key(comp: CompId, element: &str) -> StateKey {
    let mut h = Fnv::new();
    h.bytes(b"n7rt-phys-state");
    h.u32(comp.0);
    h.str(element);
    h.finish()
}

/// Lowers `circuit`; the components of `exclude` contribute no element (their pins stay nodes). Used once with a
/// non-empty set, to make the solver forget the state of deleted then restored components.
pub(crate) fn lower(circuit: &Circuit, exclude: &BTreeSet<CompId>) -> Lowered {
    lower_operating(circuit, exclude, &BTreeMap::new())
}

/// Discrete electrical output state, separate from the mechanical document. `true` means high current,
/// SENT high, or a closed pressure contact. Missing SENT state uses its frame-average output.
pub(crate) fn lower_operating(circuit: &Circuit, exclude: &BTreeSet<CompId>, operating: &BTreeMap<CompId, bool>) -> Lowered {
    lower_defective(circuit, exclude, operating, &BTreeMap::new())
}

/// [`lower_operating`] with hidden wire defects (`defects` on existing wires): a defective wire joins no net, and a
/// series defect adds its resistance between the two pins, after every component element.
pub(crate) fn lower_defective(
    circuit: &Circuit,
    exclude: &BTreeSet<CompId>,
    operating: &BTreeMap<CompId, bool>,
    defects: &BTreeMap<WireId, WireDefect>,
) -> Lowered {
    let netlist = circuit.netlist_excluding(&defects.keys().copied().collect());
    let mut node_of = BTreeMap::new();
    let mut ground = None;
    for (i, net) in netlist.nets.iter().enumerate() {
        if net.ground && ground.is_none() {
            ground = Some(i);
        }
        for p in &net.pins {
            node_of.insert(p.clone(), i);
        }
    }
    let mut b = Builder { nodes: netlist.nets.len(), node_of, elements: Vec::new(), gates: Vec::new(), comps: BTreeMap::new() };
    for (id, comp) in circuit.components() {
        if !exclude.contains(id) {
            b.component(circuit, comp, operating);
        }
    }
    for (id, defect) in defects {
        if let (Some(w), WireDefect::Series { ohm }) = (circuit.wires().get(id), defect) {
            if let (Some(a), Some(c)) = (b.node_of.get(&w.a).copied(), b.node_of.get(&w.b).copied()) {
                b.elements.push(Element::Resistor { a, b: c, r: *ohm, leakage: false, gate: None });
            }
        }
    }
    Lowered {
        network: Network { nodes: b.nodes, ground, elements: b.elements, gates: b.gates },
        netlist,
        node_of: b.node_of,
        comps: b.comps,
    }
}

struct Builder {
    nodes: usize,
    node_of: BTreeMap<PinRef, NodeId>,
    elements: Vec<Element>,
    gates: Vec<Gate>,
    comps: BTreeMap<CompId, CompLow>,
}

impl Builder {
    fn pin(&mut self, comp: CompId, id: &'static str) -> NodeId {
        let key = PinRef { comp, pin: id.to_string() };
        if let Some(n) = self.node_of.get(&key) {
            return *n;
        }
        // Every pin belongs to a net (model invariant); a missing one still gets its own isolated node.
        let n = self.nodes;
        self.nodes += 1;
        self.node_of.insert(key, n);
        n
    }

    fn internal(&mut self, low: &mut CompLow) -> NodeId {
        let n = self.nodes;
        self.nodes += 1;
        low.internal.push(n);
        n
    }

    fn internal_named(&mut self, low: &mut CompLow, name: &'static str) -> NodeId {
        let n = self.internal(low);
        low.named.push((name, n));
        n
    }

    fn capacitor(&mut self, low: &mut CompLow, name: &'static str, a: (Option<&'static str>, NodeId), b: (Option<&'static str>, NodeId), c: f64, comp: CompId) {
        self.passive(low, name, Element::Capacitor { a: a.1, b: b.1, c, key: state_key(comp, name) }, a, b);
    }

    /// Hysteresis gate on V(cp) − V(cn) from two thresholds (hidden values): a pair that is not ordered keeps the
    /// upper one and the smallest hysteresis, so that the network stays valid.
    fn gate(&mut self, comp: CompId, name: &str, cp: NodeId, cn: NodeId, on_above: f64, off_below: f64) -> GateId {
        let off_below = if off_below < on_above { off_below } else { on_above - GATE_MIN_HYSTERESIS };
        let g = self.gates.len();
        self.gates.push(Gate { cp, cn, on_above, off_below, key: state_key(comp, name) });
        g
    }

    /// ECU APPS (ch. 10 §2.2 of the mission spec, values from hidden parameters): R_ECU between LV_IN and GND, a
    /// reset gate on LV_IN − GND that carries the two current-limited one-quadrant sensor rails and the bus side of
    /// the transceiver; per channel the SIG divider to GND with its anti-aliasing capacitor at the midpoint, and
    /// the rail measurement divider; GND_S1 and GND_S2 tied to GND.
    fn ecu_apps(&mut self, low: &mut CompLow, c: &Component) {
        let id = c.id;
        let lv = (Some("lv_in"), self.pin(id, "lv_in"));
        let gnd = (Some("gnd"), self.pin(id, "gnd"));
        self.resistor(low, "r_ecu", lv, gnd, param_num(c, "r_ecu"), false, None);
        let g = self.gate(id, "por", lv.1, gnd.1, param_num(c, "reset_on"), param_num(c, "reset_off"));
        low.gate = Some(g);
        let channels = [
            ("v5_1", "sig_1", "gnd_s1", "rail_1", "r_sig_high_1", "r_sig_low_1", "c_sig_1", "r_sup_high_1", "r_sup_low_1", "tie_1", "sig1", "sup1"),
            ("v5_2", "sig_2", "gnd_s2", "rail_2", "r_sig_high_2", "r_sig_low_2", "c_sig_2", "r_sup_high_2", "r_sup_low_2", "tie_2", "sig2", "sup2"),
        ];
        for (v5, sig, gs, rail, rsh, rsl, cs, rph, rpl, tie, ch_sig, ch_sup) in channels {
            let v5 = (Some(v5), self.pin(id, v5));
            let sig = (Some(sig), self.pin(id, sig));
            let gs = (Some(gs), self.pin(id, gs));
            let e = Element::Supply {
                pos: v5.1,
                neg: gnd.1,
                v: param_num(c, "rail_v"),
                r_out: param_num(c, "rail_r_out"),
                i_lim: Some(param_num(c, "rail_i_lim")),
                key: state_key(id, rail),
                gate: Some(g),
            };
            self.source(low, rail, e, v5, gnd);
            let mid_sig = (None, self.internal_named(low, ch_sig));
            self.resistor(low, rsh, sig, mid_sig, param_num(c, "sig_r_high"), false, None);
            self.resistor(low, rsl, mid_sig, gnd, param_num(c, "sig_r_low"), false, None);
            self.capacitor(low, cs, mid_sig, gnd, param_num(c, "sig_c"), id);
            let mid_sup = (None, self.internal_named(low, ch_sup));
            self.resistor(low, rph, v5, mid_sup, param_num(c, "sup_r_high"), false, None);
            self.resistor(low, rpl, mid_sup, gnd, param_num(c, "sup_r_low"), false, None);
            self.resistor(low, tie, gs, gnd, ECU_APPS_GND_TIE, false, None);
        }
        // Bus side of the transceiver: input T network always, recessive bias out of reset only.
        let canh = (Some("can_h"), self.pin(id, "can_h"));
        let canl = (Some("can_l"), self.pin(id, "can_l"));
        let bias = (None, self.internal_named(low, "can_bias"));
        self.resistor(low, "r_in_h", canh, bias, ECU_APPS_XCVR_R_IN, false, None);
        self.resistor(low, "r_in_l", canl, bias, ECU_APPS_XCVR_R_IN, false, None);
        let e = Element::VSource { pos: bias.1, neg: gnd.1, v: ECU_APPS_BIAS_V, r_out: XCVR_BIAS_R_OUT, gate: Some(g) };
        self.source(low, "bias_drv", e, bias, gnd);
    }

    fn ecu_base(&mut self, low: &mut CompLow, c: &Component) -> (NodeId, NodeId, GateId) {
        let lv=(Some("lv_in"),self.pin(c.id,"lv_in")); let gnd=(Some("gnd"),self.pin(c.id,"gnd"));
        self.resistor(low,"r_ecu",lv,gnd,param_num(c,"r_ecu"),false,None);
        let gate=self.gate(c.id,"por",lv.1,gnd.1,param_num(c,"reset_on"),param_num(c,"reset_off"));
        low.gate=Some(gate);
        (lv.1,gnd.1,gate)
    }

    fn ecu_bus(&mut self, low:&mut CompLow,c:&Component,gnd:NodeId,gate:GateId) {
        let h=(Some("can_h"),self.pin(c.id,"can_h")); let l=(Some("can_l"),self.pin(c.id,"can_l"));
        let bias=(None,self.internal_named(low,"can_bias"));
        self.resistor(low,"r_in_h",h,bias,ECU_APPS_XCVR_R_IN,false,None);
        self.resistor(low,"r_in_l",l,bias,ECU_APPS_XCVR_R_IN,false,None);
        self.source(low,"bias_drv",Element::VSource {pos:bias.1,neg:gnd,v:ECU_APPS_BIAS_V,
            r_out:XCVR_BIAS_R_OUT,gate:Some(gate)},bias,(Some("gnd"),gnd));
    }

    fn rail(&mut self,low:&mut CompLow,c:&Component,pos:(Option<&'static str>,NodeId),gnd:NodeId,gate:GateId,name:&'static str) {
        self.source(low,name,Element::Supply {pos:pos.1,neg:gnd,v:param_num(c,"rail_v"),
            r_out:param_num(c,"rail_r_out"),i_lim:Some(param_num(c,"rail_i_lim")),key:state_key(c.id,name),gate:Some(gate)},
            pos,(Some("gnd"),gnd));
    }

    fn divider(&mut self,low:&mut CompLow,c:&Component,input:(Option<&'static str>,NodeId),gnd:NodeId,
        names:(&'static str,&'static str,&'static str),rhi:&str,rlo:&str,cap:Option<&str>) {
        let mid=(None,self.internal_named(low,names.0)); let ground=(Some("gnd"),gnd);
        self.resistor(low,names.1,input,mid,param_num(c,rhi),false,None);
        self.resistor(low,names.2,mid,ground,param_num(c,rlo),false,None);
        if let Some(name)=cap { let capacitance=param_num(c,name); if capacitance>0.0 { self.capacitor(low,names.0,mid,ground,capacitance,c.id); } }
    }

    fn ecu_wsf(&mut self,low:&mut CompLow,c:&Component) {
        let (lv,gnd,gate)=self.ecu_base(low,c);
        for (sup,input,rail,sense) in [("ws_l_sup","ws_l_in","rail_l","r_sense_l"),("ws_r_sup","ws_r_in","rail_r","r_sense_r")] {
            let sup=(Some(sup),self.pin(c.id,sup)); let input=(Some(input),self.pin(c.id,input));
            // A 0 V one-quadrant source between output and LV is a limited series branch.
            // Its output current is exactly the current drawn from LV, including in CC.
            self.source(low,rail,Element::Supply {pos:sup.1,neg:lv,v:0.0,r_out:param_num(c,"rail_r_out"),
                i_lim:Some(param_num(c,"rail_i_lim")),key:state_key(c.id,rail),gate:Some(gate)},sup,(Some("lv_in"),lv));
            self.resistor(low,sense,input,(Some("gnd"),gnd),param_num(c,"r_sense"),false,None);
        }
        self.ecu_bus(low,c,gnd,gate);
    }

    fn ecu_bps(&mut self,low:&mut CompLow,c:&Component) {
        let (_,gnd,gate)=self.ecu_base(low,c);
        let rail=(None,self.internal_named(low,"sensor_rail"));
        self.rail(low,c,rail,gnd,gate,"rail_5v");
        for (v5,sig,gs,ch,hi,lo) in [("v5_f","sig_f","gnd_f","sig_f","sig_f_hi","sig_f_lo"),
            ("v5_r","sig_r","gnd_r","sig_r","sig_r_hi","sig_r_lo")] {
            let v5=(Some(v5),self.pin(c.id,v5)); let sig=(Some(sig),self.pin(c.id,sig)); let gs=(Some(gs),self.pin(c.id,gs));
            self.resistor(low,v5.0.unwrap(),v5,rail,ECU_APPS_GND_TIE,false,None);
            self.resistor(low,gs.0.unwrap(),gs,(Some("gnd"),gnd),ECU_APPS_GND_TIE,false,None);
            self.divider(low,c,sig,gnd,(ch,hi,lo),"sig_r_high","sig_r_low",Some("sig_c"));
        }
        self.divider(low,c,rail,gnd,("sup5v","sup_hi","sup_lo"),"sup_r_high","sup_r_low",None);
        let logic=(None,self.internal_named(low,"switch_logic")); let sw=(Some("sw_in"),self.pin(c.id,"sw_in"));
        self.source(low,"switch_supply",Element::VSource {pos:logic.1,neg:gnd,v:param_num(c,"sw_logic_v"),
            r_out:XCVR_BIAS_R_OUT,gate:Some(gate)},logic,(Some("gnd"),gnd));
        self.resistor(low,"sw_pullup",logic,sw,param_num(c,"sw_pullup_ohm"),false,None);
        let ret=(Some("sw_ret"),self.pin(c.id,"sw_ret"));
        self.resistor(low,"sw_ret",ret,(Some("gnd"),gnd),ECU_APPS_GND_TIE,false,None);
        self.ecu_bus(low,c,gnd,gate);
    }

    fn ecu_sas(&mut self,low:&mut CompLow,c:&Component) {
        let (_,gnd,gate)=self.ecu_base(low,c); let supply=(Some("s5v"),self.pin(c.id,"s5v"));
        self.rail(low,c,supply,gnd,gate,"rail_5v");
        let sgnd=(Some("sgnd"),self.pin(c.id,"sgnd"));
        self.resistor(low,"sgnd_tie",sgnd,(Some("gnd"),gnd),ECU_APPS_GND_TIE,false,None);
        self.divider(low,c,supply,gnd,("sup5v","sup_hi","sup_lo"),"sup_r_top","sup_r_bottom",None);
        let sent=(Some("sent"),self.pin(c.id,"sent"));
        self.resistor(low,"sent_pullup",supply,sent,param_num(c,"pullup_r"),false,None);
        let mid=(None,self.internal_named(low,"sent_mid")); let receiver=(None,self.internal_named(low,"sent_rx"));
        self.resistor(low,"sent_filter_1",sent,mid,param_num(c,"sig_r_top"),false,None);
        self.resistor(low,"sent_filter_2",mid,receiver,param_num(c,"sig_r_bottom"),false,None);
        for (point,name,param) in [(mid,"sent_c_1","sig_c"),(receiver,"sent_c_2","c_receiver")] {
            let capacitance=param_num(c,param); if capacitance>0.0 {self.capacitor(low,name,point,(Some("gnd"),gnd),capacitance,c.id);}
        }
        self.ecu_bus(low,c,gnd,gate);
    }

    fn ats(&mut self,low:&mut CompLow,c:&Component,high:bool) {
        let p=super::wsf::SensorParams::from_component(c);
        let vcc=(Some("vcc"),self.pin(c.id,"vcc")); let gnd=(Some("gnd"),self.pin(c.id,"gnd"));
        let current=if high {p.i_high} else {p.i_low};
        // 0 V source oriented against VDD: a regulated load, continuous below the compliance voltage.
        self.source(low,"current_load",Element::Supply {pos:gnd.1,neg:vcc.1,v:0.0,r_out:p.vdd_min/current,
            i_lim:Some(current),key:state_key(c.id,"current_load"),gate:None},gnd,vcc);
        let inverse=self.gate(c.id,"reverse",gnd.1,vcc.1,0.0,-GATE_MIN_HYSTERESIS);
        self.resistor(low,"reverse_load",vcc,gnd,p.r_reverse,false,Some(inverse));
    }

    fn tle(&mut self,low:&mut CompLow,c:&Component,high:Option<bool>) {
        let vdd=(Some("vdd"),self.pin(c.id,"vdd")); let gnd=(Some("gnd"),self.pin(c.id,"gnd")); let out=(Some("out"),self.pin(c.id,"out"));
        let current=param_num(c,"consumption_a");
        self.source(low,"current_load",Element::Supply {pos:gnd.1,neg:vdd.1,v:0.0,
            r_out:param_num(c,"reset_on")/current,i_lim:Some(current),key:state_key(c.id,"current_load"),gate:None},gnd,vdd);
        let por=self.gate(c.id,"por",vdd.1,gnd.1,param_num(c,"reset_on"),param_num(c,"reset_off")); low.dies.push(por);
        let upper=self.gate(c.id,"voltage_guard",gnd.1,vdd.1,-param_num(c,"overvoltage_v"),-param_num(c,"overvoltage_v")-GATE_MIN_HYSTERESIS);
        let output_guard=self.gate(c.id,"output_guard",vdd.1,out.1,-param_num(c,"out_overvoltage_margin_v"),
            -param_num(c,"out_overvoltage_margin_v")-GATE_MIN_HYSTERESIS);
        let drive=(None,self.internal_named(low,"sent_drive"));
        let stage=(None,self.internal_named(low,"sent_stage"));
        let rout=param_num(c,"r_out")/3.0;
        self.source(low,"sent_drv",Element::Vcvs {pos:drive.1,neg:gnd.1,cp:vdd.1,cn:gnd.1,
            // the live solution shows the line's average over a frame; the frame generator uses the low and high
            // operating points
            gain:high.map_or(SENT_HIGH_FRACTION,|h|if h {1.0}else{0.0}),r_out:rout,gate:Some(por)},drive,gnd);
        // Series gates make POR, VDD overvoltage and OUT backfeed independently high impedance.
        // In the healthy state the three series resistances still total the specified r_out.
        self.resistor(low,"sent_stage",drive,stage,rout,false,Some(upper));
        self.resistor(low,"sent_output_guard",stage,out,rout,false,Some(output_guard));
        self.resistor(low,"r_leak",out,gnd,param_num(c,"r_leak"),true,None);
        for (point,name) in [(vdd,"c_vdd"),(out,"c_out")] {let cap=param_num(c,name); if cap>0.0 {self.capacitor(low,name,point,gnd,cap,c.id);}}
    }

    fn pss(&mut self,low:&mut CompLow,circuit:&Circuit,c:&Component) {
        let us=(Some("us"),self.pin(c.id,"us")); let gnd=(Some("gnd"),self.pin(c.id,"gnd")); let sig=(Some("sig"),self.pin(c.id,"sig"));
        self.resistor(low,"r_idd",us,gnd,param_num(c,"r_idd"),false,None);
        if let Some(gain)=super::bps::ratio(circuit,c) {
            let gate=self.gate(c.id,"voltage_guard",gnd.1,us.1,-param_num(c,"abs_max_v"),-param_num(c,"abs_max_v")-GATE_MIN_HYSTERESIS);
            self.source(low,"out_drv",Element::LimitedVcvs {pos:sig.1,neg:gnd.1,cp:us.1,cn:gnd.1,gain,
                r_out:param_num(c,"r_out"),i_lim:param_num(c,"output_i_lim"),key:state_key(c.id,"out_drv"),gate:Some(gate)},sig,gnd);
        }
    }

    /// Dual-die position sensor (ch. 10 §3.2, without the bidirectional current limit and the start-up timers),
    /// per die x: R_IDD and the decoupling capacitor between VDD_x and VSS_x, a start-up gate on VDD_x − VSS_x
    /// that carries the ratiometric output Vcvs OUT_x − VSS_x = TF_x(θ)/100 · (VDD_x − VSS_x) behind r_out, a
    /// permanent high-impedance leakage and the output capacitor between OUT_x and VSS_x; R_iso between the dies.
    fn mlx90377(&mut self, low: &mut CompLow, circuit: &Circuit, c: &Component) {
        let id = c.id;
        let theta = travel_percent(circuit, c);
        let dies = [
            ("vdd1", "vss1", "out1", "r_idd_1", "c_vdd_1", "por_1", "out_drv_1", "r_leak_1", "c_out_1", 1u8),
            ("vdd2", "vss2", "out2", "r_idd_2", "c_vdd_2", "por_2", "out_drv_2", "r_leak_2", "c_out_2", 2u8),
        ];
        for (vdd, vss, out, r_idd, c_vdd, por, drv, leak, c_out, die) in dies {
            let vdd = (Some(vdd), self.pin(id, vdd));
            let vss = (Some(vss), self.pin(id, vss));
            let out = (Some(out), self.pin(id, out));
            self.resistor(low, r_idd, vdd, vss, param_num(c, "r_idd"), false, None);
            self.capacitor(low, c_vdd, vdd, vss, param_num(c, "c_vdd"), id);
            let g = self.gate(id, por, vdd.1, vss.1, param_num(c, "por_on"), param_num(c, "por_off"));
            low.dies.push(g);
            let gain = transfer_percent(c, die, theta) / 100.0;
            let limit=param_num(c,"output_i_lim");
            let e = if limit>0.0 {Element::LimitedVcvs {pos:out.1,neg:vss.1,cp:vdd.1,cn:vss.1,gain,
                r_out:param_num(c,"r_out"),i_lim:limit,key:state_key(id,drv),gate:Some(g)}}
            else {Element::Vcvs { pos: out.1, neg: vss.1, cp: vdd.1, cn: vss.1, gain, r_out: param_num(c, "r_out"), gate: Some(g) }};
            self.source(low, drv, e, out, vss);
            self.resistor(low, leak, out, vss, param_num(c, "r_leak"), true, None);
            self.capacitor(low, c_out, out, vss, param_num(c, "c_out"), id);
        }
        let vss1 = (Some("vss1"), self.pin(id, "vss1"));
        let vss2 = (Some("vss2"), self.pin(id, "vss2"));
        self.resistor(low, "r_iso", vss1, vss2, param_num(c, "r_iso"), true, None);
    }

    fn push(&mut self, low: &mut CompLow, name: &'static str, element: Element, terms: [Term; 2]) {
        low.elems.push(ElemInfo { index: self.elements.len(), name, terms });
        self.elements.push(element);
    }

    /// Resistor or capacitor: its current flows from `a` to `b`, so it enters the component at `a`.
    fn passive(&mut self, low: &mut CompLow, name: &'static str, element: Element, a: (Option<&'static str>, NodeId), b: (Option<&'static str>, NodeId)) {
        let terms = [Term { pin: a.0, node: a.1, sign: 1.0 }, Term { pin: b.0, node: b.1, sign: -1.0 }];
        self.push(low, name, element, terms);
    }

    /// Source or controlled source: its current leaves by `pos` and comes back by `neg`.
    fn source(&mut self, low: &mut CompLow, name: &'static str, element: Element, pos: (Option<&'static str>, NodeId), neg: (Option<&'static str>, NodeId)) {
        let terms = [Term { pin: pos.0, node: pos.1, sign: -1.0 }, Term { pin: neg.0, node: neg.1, sign: 1.0 }];
        self.push(low, name, element, terms);
    }

    fn resistor(&mut self, low: &mut CompLow, name: &'static str, a: (Option<&'static str>, NodeId), b: (Option<&'static str>, NodeId), r: f64, leakage: bool, gate: Option<GateId>) {
        self.passive(low, name, Element::Resistor { a: a.1, b: b.1, r, leakage, gate }, a, b);
    }

    fn component(&mut self, circuit: &Circuit, c: &Component, operating: &BTreeMap<CompId, bool>) {
        let id = c.id;
        let mut low = CompLow::default();
        match c.kind {
            Kind::DcSupply => {
                if state_bool(circuit, c, "on") {
                    let pos = (Some("pos"), self.pin(id, "pos"));
                    let neg = (Some("neg"), self.pin(id, "neg"));
                    let e = Element::Supply {
                        pos: pos.1,
                        neg: neg.1,
                        v: param_num(c, "voltage"),
                        r_out: param_num(c, "r_out"),
                        i_lim: Some(param_num(c, "current_limit")),
                        key: state_key(id, "supply"),
                        gate: None,
                    };
                    self.source(&mut low, "supply", e, pos, neg);
                }
            }
            Kind::Resistor | Kind::Termination => {
                let (p1, p2) = ((Some("p1"), self.pin(id, "p1")), (Some("p2"), self.pin(id, "p2")));
                self.resistor(&mut low, "r", p1, p2, param_num(c, "resistance"), false, None);
            }
            Kind::Capacitor => {
                let (p1, p2) = ((Some("p1"), self.pin(id, "p1")), (Some("p2"), self.pin(id, "p2")));
                let e = Element::Capacitor { a: p1.1, b: p2.1, c: param_num(c, "capacitance"), key: state_key(id, "c") };
                self.passive(&mut low, "c", e, p1, p2);
            }
            Kind::Potentiometer => {
                let r = param_num(c, "resistance");
                let alpha = pot_alpha(circuit, c);
                let a = (Some("a"), self.pin(id, "a"));
                let w = (Some("w"), self.pin(id, "w"));
                let b = (Some("b"), self.pin(id, "b"));
                self.resistor(&mut low, "r_aw", a, w, (alpha * r).max(POT_FLOOR), false, None);
                self.resistor(&mut low, "r_wb", w, b, ((1.0 - alpha) * r).max(POT_FLOOR), false, None);
            }
            Kind::PushButton | Kind::Switch => {
                let closed = contact_closed(circuit, c);
                let (p1, p2) = ((Some("p1"), self.pin(id, "p1")), (Some("p2"), self.pin(id, "p2")));
                let r = if closed { param_num(c, "r_on") } else { param_num(c, "r_off") };
                self.resistor(&mut low, "contact", p1, p2, r, !closed, None);
            }
            Kind::AdcInput => {
                let r_in = param_num(c, "r_in");
                if r_in.is_finite() {
                    let (i, rf) = ((Some("in"), self.pin(id, "in")), (Some("ref"), self.pin(id, "ref")));
                    // A finite input resistance is a real conductance that references the input, not a leakage.
                    self.resistor(&mut low, "r_in", i, rf, r_in, false, None);
                }
            }
            Kind::EcuStm32 => {
                if ecu_powered(circuit, c) {
                    let gnd = (Some("gnd"), self.pin(id, "gnd"));
                    let v5 = (Some("v5"), self.pin(id, "v5"));
                    let v3 = (Some("v3v3"), self.pin(id, "v3v3"));
                    let rail5 = Element::Supply {
                        pos: v5.1,
                        neg: gnd.1,
                        v: ECU_5V,
                        r_out: param_num(c, "r_out_5v"),
                        i_lim: Some(ECU_5V_ILIM),
                        key: state_key(id, "rail_5v"),
                        gate: None,
                    };
                    self.source(&mut low, "rail_5v", rail5, v5, gnd);
                    let rail3 = Element::Supply {
                        pos: v3.1,
                        neg: gnd.1,
                        v: ECU_3V3,
                        r_out: param_num(c, "r_out_3v3"),
                        i_lim: Some(ECU_3V3_ILIM),
                        key: state_key(id, "rail_3v3"),
                        gate: None,
                    };
                    self.source(&mut low, "rail_3v3", rail3, v3, gnd);
                    if ecu_can(c) {
                        let tx = (Some("can_tx"), self.pin(id, "can_tx"));
                        let e = Element::Vcvs { pos: tx.1, neg: gnd.1, cp: v3.1, cn: gnd.1, gain: 1.0, r_out: ECU_PD1_R_OUT, gate: None };
                        self.source(&mut low, "pd1", e, tx, gnd);
                    }
                }
            }
            Kind::CanTransceiver => {
                let r_in = param_num(c, "r_in");
                let txd = (Some("txd"), self.pin(id, "txd"));
                let vss = (Some("vss"), self.pin(id, "vss"));
                let vdd = (Some("vdd"), self.pin(id, "vdd"));
                let rxd = (Some("rxd"), self.pin(id, "rxd"));
                let vref = (Some("vref"), self.pin(id, "vref"));
                let canl = (Some("canl"), self.pin(id, "canl"));
                let canh = (Some("canh"), self.pin(id, "canh"));
                let bias = (None, self.internal(&mut low));
                // Permanent: input T network, TXD pull-up, supply current as a resistance (present unpowered too).
                self.resistor(&mut low, "r_in_h", canh, bias, r_in, false, None);
                self.resistor(&mut low, "r_in_l", canl, bias, r_in, false, None);
                self.resistor(&mut low, "txd_pullup", txd, vdd, XCVR_TXD_PULLUP, false, None);
                self.resistor(&mut low, "r_dd", vdd, vss, XCVR_R_DD, false, None);
                let g = self.gates.len();
                self.gates.push(Gate { cp: vdd.1, cn: vss.1, on_above: XCVR_POR_ON, off_below: XCVR_POR_OFF, key: state_key(id, "por") });
                low.gate = Some(g);
                // Gated by the power-on reset: recessive bias, RXD high, VREF.
                let vcvs = |pos: NodeId, gain: f64, r_out: f64| Element::Vcvs { pos, neg: vss.1, cp: vdd.1, cn: vss.1, gain, r_out, gate: Some(g) };
                self.source(&mut low, "bias_drv", vcvs(bias.1, 0.5, XCVR_BIAS_R_OUT), bias, vss);
                self.source(&mut low, "rxd_drv", vcvs(rxd.1, 1.0, XCVR_RXD_R_OUT), rxd, vss);
                self.source(&mut low, "vref_drv", vcvs(vref.1, 0.5, XCVR_VREF_R_OUT), vref, vss);
            }
            Kind::CanBus => {
                let canh = (Some("canh"), self.pin(id, "canh"));
                let canl = (Some("canl"), self.pin(id, "canl"));
                let gnd = (Some("gnd"), self.pin(id, "gnd"));
                if param_bool(c, "remote_termination") {
                    self.resistor(&mut low, "term", canh, canl, BUS_TERMINATION, false, None);
                }
                let n = param_int(c, "remote_nodes");
                if n >= 1 {
                    let r = param_num(c, "remote_r_in") / n as f64;
                    let rbias = (None, self.internal(&mut low));
                    self.resistor(&mut low, "r_h", canh, rbias, r, false, None);
                    self.resistor(&mut low, "r_l", canl, rbias, r, false, None);
                    let e = Element::VSource { pos: rbias.1, neg: gnd.1, v: BUS_BIAS_V, r_out: BUS_BIAS_R_OUT, gate: None };
                    self.source(&mut low, "bias", e, rbias, gnd);
                }
            }
            Kind::EcuApps => self.ecu_apps(&mut low, c),
            Kind::Mlx90377 => self.mlx90377(&mut low, circuit, c),
            Kind::EcuWsf => self.ecu_wsf(&mut low, c),
            Kind::EcuSas => self.ecu_sas(&mut low, c),
            Kind::EcuBps => self.ecu_bps(&mut low, c),
            Kind::Ats682Lsh => self.ats(&mut low, c, operating.get(&id).copied().unwrap_or(true)),
            Kind::Tle5014 => self.tle(&mut low, c, operating.get(&id).copied()),
            Kind::Pss260 => self.pss(&mut low, circuit, c),
            Kind::PressureSwitch => {
                let closed = operating.get(&id).copied().unwrap_or(false);
                let p1=(Some("p1"),self.pin(id,"p1")); let p2=(Some("p2"),self.pin(id,"p2"));
                self.resistor(&mut low,"contact",p1,p2,param_num(c,if closed {"r_on"} else {"r_off"}),!closed,None);
            }
            Kind::Ground | Kind::Junction | Kind::FrontAxle | Kind::SteeringColumn | Kind::BrakeHydraulics => {}
        }
        self.comps.insert(id, low);
    }
}

// ------------------------------------------------------------------------------------- parameters and states

fn param_default(kind: Kind, name: &str) -> Option<Lit> {
    kind_def(kind).params.iter().find(|p| p.name == name).map(|p| p.default)
}

fn state_default(kind: Kind, name: &str) -> Option<Lit> {
    kind_def(kind).states.iter().find(|s| s.name == name).map(|s| s.default)
}

fn lit_num(l: Option<Lit>) -> f64 {
    match l {
        Some(Lit::Num(x)) => x,
        Some(Lit::Int(i)) => i as f64,
        Some(Lit::Bool(b)) => f64::from(u8::from(b)),
        Some(Lit::Choice(s)) => s.parse().unwrap_or(0.0),
        None => 0.0,
    }
}

fn value_num(v: &Value) -> Option<f64> {
    match v {
        Value::Num(x) => Some(*x),
        Value::Int(i) => Some(*i as f64),
        Value::Choice(s) if s == "inf" || s == "\u{221e}" => Some(f64::INFINITY),
        Value::Choice(s) => s.parse().ok(),
        Value::Bool(_) => None,
    }
}

/// A number parameter (+∞ for an ideal ADC input); the library default if absent or mistyped (the model
/// invariants make both impossible).
pub(crate) fn param_num(c: &Component, name: &str) -> f64 {
    c.params.get(name).and_then(value_num).unwrap_or_else(|| lit_num(param_default(c.kind, name)))
}

/// An integer parameter; a Choice of digits ("12") is read as its number.
pub(crate) fn param_int(c: &Component, name: &str) -> i64 {
    let v = c.params.get(name).and_then(value_num).unwrap_or_else(|| lit_num(param_default(c.kind, name)));
    if v.is_finite() {
        v as i64
    } else {
        0
    }
}

pub(crate) fn param_bool(c: &Component, name: &str) -> bool {
    match c.params.get(name) {
        Some(Value::Bool(b)) => *b,
        _ => matches!(param_default(c.kind, name), Some(Lit::Bool(true))),
    }
}

pub(crate) fn param_choice<'a>(c: &'a Component, name: &str) -> &'a str {
    match c.params.get(name) {
        Some(Value::Choice(s)) => s.as_str(),
        _ => match param_default(c.kind, name) {
            Some(Lit::Choice(s)) => s,
            _ => "",
        },
    }
}

pub(crate) fn state_bool(circuit: &Circuit, c: &Component, name: &str) -> bool {
    match circuit.state(c.id).and_then(|m| m.get(name)) {
        Some(Value::Bool(b)) => *b,
        _ => matches!(state_default(c.kind, name), Some(Lit::Bool(true))),
    }
}

pub(crate) fn state_num(circuit: &Circuit, c: &Component, name: &str) -> f64 {
    circuit
        .state(c.id)
        .and_then(|m| m.get(name))
        .and_then(value_num)
        .unwrap_or_else(|| lit_num(state_default(c.kind, name)))
}

/// Wiper position α = k / 1000, k integer (contract §2.1).
pub(crate) fn pot_alpha(circuit: &Circuit, c: &Component) -> f64 {
    let p = state_num(circuit, c, "position");
    let k = if p.is_finite() { (p * 1000.0).round().clamp(0.0, 1000.0) } else { 500.0 };
    k / 1000.0
}

/// Pedal travel θ of a position sensor in percent, θ = 100·position with position = k/1000 (k integral): θ = k/10.
pub(crate) fn travel_percent(circuit: &Circuit, c: &Component) -> f64 {
    let p = state_num(circuit, c, "position");
    let k = if p.is_finite() { (p * 1000.0).round().clamp(0.0, 1000.0) } else { 0.0 };
    k / 10.0
}

/// Transfer function of die `die` (1 or 2) of a position sensor, in %VDD: offset + slope·θ, clamped to
/// [clamp_low, clamp_high] (hidden parameters).
pub(crate) fn transfer_percent(c: &Component, die: u8, theta: f64) -> f64 {
    let p = |name: &str| param_num(c, &format!("tf{die}_{name}"));
    (p("offset") + p("slope") * theta).max(p("clamp_low")).min(p("clamp_high"))
}

/// State of a gate in one computation (None: no such gate, or its island has no result).
pub(crate) fn gate_state(gates: &[Option<bool>], which: Option<GateId>) -> Option<bool> {
    which.and_then(|g| gates.get(g).copied().flatten())
}

/// Closed contact: switch `closed`; push button (momentary ? pressed : latched) XOR (contact = nc).
pub(crate) fn contact_closed(circuit: &Circuit, c: &Component) -> bool {
    match c.kind {
        Kind::Switch => state_bool(circuit, c, "closed"),
        Kind::PushButton => {
            let actuated =
                if param_choice(c, "mode") == "latching" { state_bool(circuit, c, "latched") } else { state_bool(circuit, c, "pressed") };
            actuated != (param_choice(c, "contact") == "nc")
        }
        _ => false,
    }
}

pub(crate) fn ecu_powered(circuit: &Circuit, c: &Component) -> bool {
    state_bool(circuit, c, "usb")
}

/// PD1 driven by FDCAN (`can`), or high impedance (`hi_z`, runtime image without FDCAN).
pub(crate) fn ecu_can(c: &Component) -> bool {
    param_choice(c, "can_mode") != "hi_z"
}

#[cfg(test)]
mod tests {
    //! Element lists of every kind (contract §2.1). These tests build circuits through the document API, so they
    //! need the n7rt-circuit implementation.

    use super::*;
    use n7rt_circuit::{Document, Edit, Interact, Pt, Rot, ValueMap};

    fn place(doc: &mut Document, kind: Kind, x: i32, params: &[(&str, Value)]) -> CompId {
        let params: ValueMap = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let edit = Edit::AddComponent { kind, pos: Pt { x, y: 0 }, rot: Rot::R0, params, label: None };
        let applied = doc.edit(doc.rev(), &edit).expect("component placed");
        applied.patch.components.iter().find(|(_, b, a)| b.is_none() && a.is_some()).map(|(id, _, _)| *id).expect("new id")
    }

    fn one(kind: Kind, params: &[(&str, Value)]) -> (Circuit, CompId) {
        let mut doc = Document::new(Circuit::new("t"));
        let id = place(&mut doc, kind, 0, params);
        (doc.circuit().clone(), id)
    }

    fn node(l: &Lowered, comp: CompId, pin: &str) -> NodeId {
        l.node_of[&PinRef { comp, pin: pin.to_string() }]
    }

    #[test]
    fn resistor_termination_capacitor() {
        let (c, id) = one(Kind::Resistor, &[("resistance", Value::Num(4700.0))]);
        let l = lower(&c, &BTreeSet::new());
        let (p1, p2) = (node(&l, id, "p1"), node(&l, id, "p2"));
        assert_eq!(l.network.elements, vec![Element::Resistor { a: p1, b: p2, r: 4700.0, leakage: false, gate: None }]);
        assert_eq!(l.network.nodes, 2);
        assert_eq!(l.network.ground, None);

        let (c, id) = one(Kind::Termination, &[]);
        let l = lower(&c, &BTreeSet::new());
        assert!(matches!(l.network.elements[..], [Element::Resistor { r, leakage: false, gate: None, .. }] if r == 120.0));
        let _ = id;

        let (c, id) = one(Kind::Capacitor, &[("capacitance", Value::Num(4.7e-6))]);
        let l = lower(&c, &BTreeSet::new());
        let (p1, p2) = (node(&l, id, "p1"), node(&l, id, "p2"));
        assert_eq!(l.network.elements, vec![Element::Capacitor { a: p1, b: p2, c: 4.7e-6, key: state_key(id, "c") }]);
    }

    #[test]
    fn supply_on_and_off() {
        let mut doc = Document::new(Circuit::new("t"));
        let id = place(&mut doc, Kind::DcSupply, 0, &[("voltage", Value::Num(12.0))]);
        let l = lower(doc.circuit(), &BTreeSet::new());
        let (pos, neg) = (node(&l, id, "pos"), node(&l, id, "neg"));
        assert_eq!(
            l.network.elements,
            vec![Element::Supply { pos, neg, v: 12.0, r_out: 0.01, i_lim: Some(0.1), key: state_key(id, "supply"), gate: None }]
        );
        doc.interact(&Interact::SetPower { comp: id, on: false }).expect("power off");
        assert!(lower(doc.circuit(), &BTreeSet::new()).network.elements.is_empty());
    }

    #[test]
    fn potentiometer_segments_and_floor() {
        let mut doc = Document::new(Circuit::new("t"));
        let id = place(&mut doc, Kind::Potentiometer, 0, &[]);
        doc.interact(&Interact::SetPot { comp: id, position: 0.3 }).expect("pot");
        let l = lower(doc.circuit(), &BTreeSet::new());
        let (a, w, b) = (node(&l, id, "a"), node(&l, id, "w"), node(&l, id, "b"));
        assert_eq!(
            l.network.elements,
            vec![
                Element::Resistor { a, b: w, r: (0.3 * 1e4f64).max(1e-3), leakage: false, gate: None },
                Element::Resistor { a: w, b, r: ((1.0 - 0.3) * 1e4f64).max(1e-3), leakage: false, gate: None },
            ]
        );
        doc.interact(&Interact::SetPot { comp: id, position: 0.0 }).expect("pot");
        let l = lower(doc.circuit(), &BTreeSet::new());
        assert!(matches!(l.network.elements[0], Element::Resistor { r, .. } if r == 1e-3));
        assert!(matches!(l.network.elements[1], Element::Resistor { r, .. } if r == 1e4));
    }

    #[test]
    fn contacts_closed_and_leakage() {
        let mut doc = Document::new(Circuit::new("t"));
        let s = place(&mut doc, Kind::Switch, 0, &[]);
        let nc = place(&mut doc, Kind::PushButton, 20, &[("contact", Value::Choice("nc".into()))]);
        let latch = place(&mut doc, Kind::PushButton, 40, &[("mode", Value::Choice("latching".into()))]);
        let contact = |doc: &Document, id: CompId| {
            let l = lower(doc.circuit(), &BTreeSet::new());
            let e = l.comps[&id].elem("contact").expect("contact").index;
            match l.network.elements[e] {
                Element::Resistor { r, leakage, .. } => (r, leakage),
                _ => panic!("contact is a resistor"),
            }
        };
        // Switch open by default: r_off as a leakage; closed: r_on.
        assert_eq!(contact(&doc, s), (1e9, true));
        doc.interact(&Interact::SetSwitch { comp: s, closed: true }).expect("close");
        assert_eq!(contact(&doc, s), (0.01, false));
        // NC button: closed at rest, open while pressed.
        assert_eq!(contact(&doc, nc), (0.1, false));
        doc.interact(&Interact::SetButton { comp: nc, pressed: true }).expect("press");
        assert_eq!(contact(&doc, nc), (1e8, true));
        // Latching button: toggles on the rising edge of `pressed` only.
        assert_eq!(contact(&doc, latch), (1e8, true));
        doc.interact(&Interact::SetButton { comp: latch, pressed: true }).expect("press");
        assert_eq!(contact(&doc, latch), (0.1, false));
        doc.interact(&Interact::SetButton { comp: latch, pressed: false }).expect("release");
        assert_eq!(contact(&doc, latch), (0.1, false));
        doc.interact(&Interact::SetButton { comp: latch, pressed: true }).expect("press");
        assert_eq!(contact(&doc, latch), (1e8, true));
    }

    #[test]
    fn adc_input_ideal_or_finite() {
        let (c, _) = one(Kind::AdcInput, &[]);
        assert!(lower(&c, &BTreeSet::new()).network.elements.is_empty(), "r_in = ∞: no element");
        let (c, id) = one(Kind::AdcInput, &[("r_in", Value::Num(1e6))]);
        let l = lower(&c, &BTreeSet::new());
        let (i, r) = (node(&l, id, "in"), node(&l, id, "ref"));
        assert_eq!(l.network.elements, vec![Element::Resistor { a: i, b: r, r: 1e6, leakage: false, gate: None }]);
    }

    #[test]
    fn ecu_rails_and_pd1() {
        let mut doc = Document::new(Circuit::new("t"));
        let id = place(&mut doc, Kind::EcuStm32, 0, &[]);
        let l = lower(doc.circuit(), &BTreeSet::new());
        let n = |p: &str| node(&l, id, p);
        assert_eq!(
            l.network.elements,
            vec![
                Element::Supply { pos: n("v5"), neg: n("gnd"), v: 5.0, r_out: 0.1, i_lim: Some(0.5), key: state_key(id, "rail_5v"), gate: None },
                Element::Supply { pos: n("v3v3"), neg: n("gnd"), v: 3.3, r_out: 0.05, i_lim: Some(1.3), key: state_key(id, "rail_3v3"), gate: None },
                Element::Vcvs { pos: n("can_tx"), neg: n("gnd"), cp: n("v3v3"), cn: n("gnd"), gain: 1.0, r_out: 50.0, gate: None },
            ]
        );
        doc.edit(doc.rev(), &Edit::SetParam { comp: id, name: "can_mode".into(), value: Value::Choice("hi_z".into()) }).expect("hi_z");
        assert_eq!(lower(doc.circuit(), &BTreeSet::new()).network.elements.len(), 2, "hi_z: no PD1 driver");
        doc.interact(&Interact::SetPower { comp: id, on: false }).expect("unpowered");
        assert!(lower(doc.circuit(), &BTreeSet::new()).network.elements.is_empty(), "usb false: no rail");
    }

    #[test]
    fn transceiver_permanent_and_gated() {
        let (c, id) = one(Kind::CanTransceiver, &[("r_in", Value::Num(30e3))]);
        let l = lower(&c, &BTreeSet::new());
        let n = |p: &str| node(&l, id, p);
        let nets = l.netlist.nets.len();
        assert_eq!(nets, 8, "one net per pin");
        let bias = nets; // first internal node, after every net
        assert_eq!(l.network.nodes, nets + 1);
        assert_eq!(l.comps[&id].internal, vec![bias]);
        assert_eq!(l.network.gates, vec![Gate { cp: n("vdd"), cn: n("vss"), on_above: 4.05, off_below: 3.70, key: state_key(id, "por") }]);
        let g = Some(0);
        assert_eq!(
            l.network.elements,
            vec![
                Element::Resistor { a: n("canh"), b: bias, r: 30e3, leakage: false, gate: None },
                Element::Resistor { a: n("canl"), b: bias, r: 30e3, leakage: false, gate: None },
                Element::Resistor { a: n("txd"), b: n("vdd"), r: 25e3, leakage: false, gate: None },
                Element::Resistor { a: n("vdd"), b: n("vss"), r: 500.0, leakage: false, gate: None },
                Element::Vcvs { pos: bias, neg: n("vss"), cp: n("vdd"), cn: n("vss"), gain: 0.5, r_out: 1e-3, gate: g },
                Element::Vcvs { pos: n("rxd"), neg: n("vss"), cp: n("vdd"), cn: n("vss"), gain: 1.0, r_out: 187.5, gate: g },
                Element::Vcvs { pos: n("vref"), neg: n("vss"), cp: n("vdd"), cn: n("vss"), gain: 0.5, r_out: 5e3, gate: g },
            ]
        );
    }

    #[test]
    fn bus_remote_nodes() {
        let (c, id) = one(Kind::CanBus, &[("remote_nodes", Value::Int(2)), ("remote_r_in", Value::Num(25e3))]);
        let l = lower(&c, &BTreeSet::new());
        let n = |p: &str| node(&l, id, p);
        let rbias = l.netlist.nets.len();
        assert_eq!(
            l.network.elements,
            vec![
                Element::Resistor { a: n("canh"), b: n("canl"), r: 120.0, leakage: false, gate: None },
                Element::Resistor { a: n("canh"), b: rbias, r: 12.5e3, leakage: false, gate: None },
                Element::Resistor { a: n("canl"), b: rbias, r: 12.5e3, leakage: false, gate: None },
                Element::VSource { pos: rbias, neg: n("gnd"), v: 2.5, r_out: 1.0, gate: None },
            ]
        );
        let (c, _) = one(Kind::CanBus, &[("remote_nodes", Value::Int(0)), ("remote_termination", Value::Bool(false))]);
        let l = lower(&c, &BTreeSet::new());
        assert!(l.network.elements.is_empty(), "no node, no termination: nothing");
        assert_eq!(l.network.nodes, 3, "no rbias node without remote nodes");
    }

    #[test]
    fn ground_and_order() {
        let mut doc = Document::new(Circuit::new("t"));
        let g = place(&mut doc, Kind::Ground, 0, &[]);
        let r2 = place(&mut doc, Kind::Resistor, 20, &[]);
        let r1 = place(&mut doc, Kind::Resistor, 40, &[]);
        let l = lower(doc.circuit(), &BTreeSet::new());
        let gnd = node(&l, g, "gnd");
        assert_eq!(l.network.ground, Some(gnd));
        // Nodes follow the NetKey order; elements follow the CompId order.
        for (i, net) in l.netlist.nets.iter().enumerate() {
            assert_eq!(node(&l, net.key.comp, &net.key.pin), i);
        }
        assert!(r2 < r1);
        assert_eq!(l.comps[&r2].elems[0].index, 0);
        assert_eq!(l.comps[&r1].elems[0].index, 1);
        // An excluded component keeps its pins as nodes but has no element.
        let ex: BTreeSet<CompId> = [r2].into_iter().collect();
        let l2 = lower(doc.circuit(), &ex);
        assert_eq!(l2.network.nodes, l.network.nodes);
        assert_eq!(l2.network.elements.len(), 1);
    }

    /// Neutral catalog values only (the mission values live in the private bench file).
    #[test]
    fn ecu_apps_black_box() {
        let (c, id) = one(Kind::EcuApps, &[]);
        let l = lower(&c, &BTreeSet::new());
        let n = |p: &str| node(&l, id, p);
        let nets = l.netlist.nets.len();
        assert_eq!(nets, 10, "one net per pin");
        let low = &l.comps[&id];
        // Internal nodes after the nets: SIG and rail midpoints per channel, then the CAN bias.
        let (s1, p1, s2, p2, bias) = (nets, nets + 1, nets + 2, nets + 3, nets + 4);
        assert_eq!(low.internal, vec![s1, p1, s2, p2, bias]);
        assert_eq!(low.named, vec![("sig1", s1), ("sup1", p1), ("sig2", s2), ("sup2", p2), ("can_bias", bias)]);
        assert_eq!(l.network.gates, vec![Gate { cp: n("lv_in"), cn: n("gnd"), on_above: 6.5, off_below: 6.0, key: state_key(id, "por") }]);
        assert_eq!(low.gate, Some(0));
        let g = Some(0);
        let r = |a, b, r| Element::Resistor { a, b, r, leakage: false, gate: None };
        let rail = |pos, key| Element::Supply { pos, neg: n("gnd"), v: 5.0, r_out: 0.05, i_lim: Some(0.1), key: state_key(id, key), gate: g };
        let cap = |a, key| Element::Capacitor { a, b: n("gnd"), c: 2.2e-7, key: state_key(id, key) };
        assert_eq!(
            l.network.elements,
            vec![
                r(n("lv_in"), n("gnd"), 1e3),
                rail(n("v5_1"), "rail_1"),
                r(n("sig_1"), s1, 15e3),
                r(s1, n("gnd"), 10e3),
                cap(s1, "c_sig_1"),
                r(n("v5_1"), p1, 20e3),
                r(p1, n("gnd"), 10e3),
                r(n("gnd_s1"), n("gnd"), 1e-3),
                rail(n("v5_2"), "rail_2"),
                r(n("sig_2"), s2, 15e3),
                r(s2, n("gnd"), 10e3),
                cap(s2, "c_sig_2"),
                r(n("v5_2"), p2, 20e3),
                r(p2, n("gnd"), 10e3),
                r(n("gnd_s2"), n("gnd"), 1e-3),
                r(n("can_h"), bias, 25e3),
                r(n("can_l"), bias, 25e3),
                Element::VSource { pos: bias, neg: n("gnd"), v: 2.5, r_out: 1e-3, gate: g },
            ]
        );
    }

    #[test]
    fn mlx90377_ratiometric_dies() {
        let mut doc = Document::new(Circuit::new("t"));
        let id = place(&mut doc, Kind::Mlx90377, 0, &[]);
        let l = lower(doc.circuit(), &BTreeSet::new());
        let n = |p: &str| node(&l, id, p);
        assert_eq!(l.comps[&id].dies, vec![0, 1]);
        assert_eq!(
            l.network.gates,
            vec![
                Gate { cp: n("vdd1"), cn: n("vss1"), on_above: 4.0, off_below: 3.8, key: state_key(id, "por_1") },
                Gate { cp: n("vdd2"), cn: n("vss2"), on_above: 4.0, off_below: 3.8, key: state_key(id, "por_2") },
            ]
        );
        let die = |x: usize, gain: f64| {
            let (vdd, vss, out) = (n(&format!("vdd{x}")), n(&format!("vss{x}")), n(&format!("out{x}")));
            let key = |k: &str| state_key(id, &format!("{k}_{x}"));
            vec![
                Element::Resistor { a: vdd, b: vss, r: 500.0, leakage: false, gate: None },
                Element::Capacitor { a: vdd, b: vss, c: 1e-7, key: key("c_vdd") },
                Element::Vcvs { pos: out, neg: vss, cp: vdd, cn: vss, gain, r_out: 10.0, gate: Some(x - 1) },
                Element::Resistor { a: out, b: vss, r: 2e6, leakage: true, gate: None },
                Element::Capacitor { a: out, b: vss, c: 4.7e-8, key: key("c_out") },
            ]
        };
        let iso = Element::Resistor { a: n("vss1"), b: n("vss2"), r: 1e7, leakage: true, gate: None };
        // At rest (default position 0): TF = offset = 10 %VDD for both dies.
        let mut expected = die(1, 0.1);
        expected.extend(die(2, 0.1));
        expected.push(iso.clone());
        assert_eq!(l.network.elements, expected);
        // θ = 100·k/1000: TF = 10 + 0.5·θ, clamped to [0, 100] %VDD.
        doc.interact(&Interact::SetPot { comp: id, position: 0.457 }).expect("travel");
        let l = lower(doc.circuit(), &BTreeSet::new());
        let gains: Vec<f64> = l.network.elements.iter().filter_map(|e| if let Element::Vcvs { gain, .. } = e { Some(*gain) } else { None }).collect();
        assert_eq!(gains, vec![(10.0 + 0.5 * 45.7) / 100.0; 2]);
        let c = doc.circuit().components()[&id].clone();
        assert_eq!(travel_percent(doc.circuit(), &c), 45.7);
        assert_eq!(transfer_percent(&c, 1, 300.0), 100.0, "clamped high");
        assert_eq!(transfer_percent(&c, 2, -300.0), 0.0, "clamped low");
    }

    #[test]
    fn keys_are_stable_and_distinct() {
        let a = state_key(CompId(3), "c");
        assert_eq!(a, state_key(CompId(3), "c"));
        assert_ne!(a, state_key(CompId(4), "c"));
        assert_ne!(state_key(CompId(3), "rail_5v"), state_key(CompId(3), "rail_3v3"));
    }

    #[test]
    fn a_live_sent_line_reads_its_frame_average() {
        // TLE5014 rev. 1.0 p. 33: 282 UT with the pause pulse, 10 pulses 5 UT low each
        assert!((SENT_HIGH_FRACTION - 0.8227).abs() < 1e-4);
        assert!((SENT_HIGH_FRACTION * 5.0 - 4.11).abs() < 0.01, "about 4.11 V on a 5 V line");
    }
}

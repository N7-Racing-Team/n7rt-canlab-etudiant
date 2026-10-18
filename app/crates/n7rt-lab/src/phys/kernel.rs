//! The deterministic core behind [`super::PhysCore`] (contract §8.1, §8.2): document, solver, frames of 1 ms,
//! journal, replay and digest. No clock, no thread: the same initial circuit and the same commands at the same
//! frames give the same digest.
//!
//! - Commands are validated in the document before the reply. In STOP their electrical effect is previewed at
//!   once; in RUN it waits for the next frame boundary, where the circuit is lowered again if something
//!   electrical changed (geometry alone changes nothing computed, only names).
//! - Journal: `(frame, command)`. In RUN, several `set_pot` on one potentiometer within a frame keep the last one;
//!   the release of a momentary button pressed during the current frame is deferred after the frame that saw the
//!   press, and journaled at the frame where it takes effect.
//! - A STOP preview also samples every ADC channel at the frozen instant (the code at 0+ of contract §9.1).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use n7rt_circuit::{
    kind_def, persist, Applied, Circuit, CompId, Component, Document, Edit, Interact, Kind, Netlist, Patch, PinRef, Refusal,
    RefusalCode, Value, WireId,
};
use n7rt_solver::{Element, Solver, SolverStats, StateKey, SupplyMode};

use super::defects::{Defects, SystemOp};
use super::meter::{energized, Meter, MeterFn};
use super::lower::{
    ecu_powered, lower, lower_defective, param_bool, param_choice, param_int, param_num, state_bool, state_num, ECU_APPS_ADC_BITS, ECU_APPS_ADC_VREF, ECU_APPS_CHANNELS,
};
use super::probe::{blocked_nodes, eval_xcvr, sample, sample_period, AdcHolds, Capture, Ctx, Frame, Probe, Results, TxdTimer,MissionHistory};
use super::util::{finite, fnv_text, recast, unavailable, Fnv};
use super::{
    AdcSample, CommandReply, CompValues, Diagnostic, Disclosure, EditReply, EngineStats, InteractReply, JournalEntry, Measure,
    NetValue, OpState, PhysCommand, PhysFrame, PinValue, PreviewReply, Reading, RunReply, RunState, SnapshotDto, SupplyOutput,
    ValuesDto,
};

#[derive(Debug)]
pub(crate) struct Core {
    doc: Document,
    solver: Solver,
    run: RunState,
    frame_index: u64,
    journal: Vec<JournalEntry>,
    /// The lowering loaded in the solver.
    ctx: Arc<Ctx>,
    needs_lower: bool,
    load_error: bool,
    /// Results of the last frame or preview.
    capture: Arc<Capture>,
    timers: BTreeMap<CompId, TxdTimer>,
    adc: AdcHolds,
    last_modes: BTreeMap<StateKey, SupplyMode>,
    operating: BTreeMap<CompId, bool>,
    rotors: BTreeMap<CompId, super::wsf::RotorState>,
    source_configs: BTreeMap<CompId, n7rt_vehicle::SasConfig>,
    sent: BTreeMap<(CompId, CompId), (String, super::sas::SentGenerator)>,
    /// Actual mechanical angle; the document state is the commanded target.
    mechanical: BTreeMap<CompId, f64>,
    mechanical_before: BTreeMap<CompId, f64>,
    prepared_inputs: BTreeMap<CompId, n7rt_vehicle::EcuInputs>,
    mission_history:MissionHistory,
    sensor_seed: u64,
    /// Components deleted since the last load: if one comes back (undo) before the solver saw it gone, its
    /// state must still be forgotten (a restored capacitor is discharged).
    deleted_since_load: BTreeSet<CompId>,
    /// Push buttons pressed and not yet seen by a frame.
    unseen_presses: BTreeSet<CompId>,
    deferred_releases: BTreeSet<CompId>,
    /// Wiper position before the journaled `set_pot` of the current frame (RUN coalescing).
    pot_prior: BTreeMap<CompId, Value>,
    doc_netlist: Netlist,
    /// Hidden wire defects of the mission engine (never in the document).
    defects: Defects,
    meter: Meter,
    disclosure: Disclosure,
    /// Diagnostics added to every frame (engine_restarted).
    notices: Vec<Diagnostic>,
    /// Added to the document revision, so that revisions keep increasing across loads and restarts.
    rev_offset: u64,
    saved_hash: u64,
    /// Increases at every document change.
    doc_gen: u64,
    /// Identity of the document given by the handle (changes when the document is replaced), sent to the UI.
    doc_id: u64,
    dirty_cache: Cell<Option<(u64, bool)>>,
    frame_cache: RefCell<Option<Arc<PhysFrame>>>,
    #[cfg(test)]
    pub(crate) corrupt_next_load: bool,
}

impl Core {
    pub fn new(circuit: Circuit) -> Core {
        let doc = Document::new(circuit);
        let doc_netlist = doc.circuit().netlist();
        let saved_hash = fnv_text(&persist::save(doc.circuit()));
        let mut core = Core {
            doc,
            solver: Solver::new(),
            run: RunState::Stop,
            frame_index: 0,
            journal: Vec::new(),
            ctx: Arc::new(Ctx::empty()),
            needs_lower: true,
            load_error: false,
            capture: Arc::new(Capture::empty()),
            timers: BTreeMap::new(),
            adc: BTreeMap::new(),
            last_modes: BTreeMap::new(),
            operating:BTreeMap::new(),rotors:BTreeMap::new(),source_configs:BTreeMap::new(),sent:BTreeMap::new(),
            mechanical:BTreeMap::new(),mechanical_before:BTreeMap::new(),prepared_inputs:BTreeMap::new(),mission_history:BTreeMap::new(),sensor_seed:0,
            deleted_since_load: BTreeSet::new(),
            unseen_presses: BTreeSet::new(),
            deferred_releases: BTreeSet::new(),
            pot_prior: BTreeMap::new(),
            doc_netlist,
            defects: Defects::default(),
            meter: Meter::default(),
            disclosure: Disclosure::Exploration,
            notices: Vec::new(),
            rev_offset: 0,
            saved_hash,
            doc_gen: 0,
            doc_id: 0,
            dirty_cache: Cell::new(None),
            frame_cache: RefCell::new(None),
            #[cfg(test)]
            corrupt_next_load: false,
        };
        core.recompute();
        core
    }

    // ------------------------------------------------------------------------------------------- accessors

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Configure only the source-side physical protocol, never a consumer or expected result.
    pub fn configure_source(&mut self,ecu:CompId,pack:&n7rt_vehicle::Pack)->Result<(),String> {
        let c=self.doc.circuit().components().get(&ecu).ok_or("ECU physique absent")?;
        if let Some(config)=pack.sas_config() {
            if c.kind!=Kind::EcuSas {return Err("configuration source incompatible avec le composant".into());}
            config.validate()?;
            for sensor in self.doc.circuit().components().values().filter(|c|c.kind==Kind::Tle5014) {
                super::sas::SentGenerator::new(sas_sensor_config(sensor,c,config),config)?;
            }
            self.source_configs.insert(ecu,config.clone());
            self.sent.retain(|(id,_),_|*id!=ecu);
        }
        self.recompute();
        Ok(())
    }

    pub fn set_sensor_seed(&mut self,seed:u64) {self.sensor_seed=seed;self.rotors.clear();}

    pub fn run_state(&self) -> RunState {
        self.run
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// Reported revision.
    pub fn rev(&self) -> u64 {
        self.doc.rev() + self.rev_offset
    }

    pub fn doc_gen(&self) -> u64 {
        self.doc_gen
    }

    pub fn solver_stats(&self) -> SolverStats {
        self.capture.res.stats.clone()
    }

    pub fn set_rev_offset(&mut self, offset: u64) {
        self.rev_offset = offset;
        self.invalidate();
    }

    pub fn set_doc_id(&mut self, doc_id: u64) {
        self.doc_id = doc_id;
    }

    pub fn set_disclosure(&mut self, disclosure: Disclosure) {
        self.disclosure = disclosure;
        self.invalidate();
    }

    pub fn set_notices(&mut self, notices: Vec<Diagnostic>) {
        self.notices = notices;
        self.invalidate();
    }

    /// Removes the `n` oldest journal entries (spilled to disk by the handle).
    pub fn drain_journal(&mut self, n: usize) {
        let n = n.min(self.journal.len());
        self.journal.drain(..n);
    }

    pub fn dirty(&self) -> bool {
        if let Some((gen, d)) = self.dirty_cache.get() {
            if gen == self.doc_gen {
                return d;
            }
        }
        let d = fnv_text(&persist::save(self.doc.circuit())) != self.saved_hash;
        self.dirty_cache.set(Some((self.doc_gen, d)));
        d
    }

    pub fn mark_saved(&mut self) {
        self.saved_hash = fnv_text(&persist::save(self.doc.circuit()));
        self.dirty_cache.set(None);
    }

    fn invalidate(&self) {
        *self.frame_cache.borrow_mut() = None;
    }

    fn touch(&mut self) {
        self.doc_gen += 1;
    }

    fn stale(&self, base_rev: u64) -> Refusal {
        Refusal {
            code: RefusalCode::StaleRevision,
            ambiguity: None,
            message_fr: format!("Révision {base_rev} périmée : le circuit est à la révision {}, rechargez-le", self.rev()),
            at: None,
            items: Vec::new(),
        }
    }

    /// Raw document revision for a reported one.
    fn check_rev(&self, base_rev: u64) -> Result<u64, Refusal> {
        if base_rev == self.rev() {
            Ok(self.doc.rev())
        } else {
            Err(self.stale(base_rev))
        }
    }

    // -------------------------------------------------------------------------------------------- commands

    pub fn preview(&self, base_rev: u64, edit: &Edit) -> Result<PreviewReply, Refusal> {
        let raw = self.check_rev(base_rev)?;
        let patch = self.doc.preview(raw, edit)?;
        Ok(PreviewReply { base_rev, patch })
    }

    pub fn apply(&mut self, command: &PhysCommand) -> Result<CommandReply, Refusal> {
        match command {
            PhysCommand::Edit { base_rev, edit } => {
                let raw = self.check_rev(*base_rev)?;
                let applied = self.doc.edit(raw, edit)?;
                Ok(self.committed(PhysCommand::Edit { base_rev: raw, edit: edit.clone() }, applied))
            }
            PhysCommand::Undo => {
                let applied = self.doc.undo()?;
                Ok(self.committed(PhysCommand::Undo, applied))
            }
            PhysCommand::Redo => {
                let applied = self.doc.redo()?;
                Ok(self.committed(PhysCommand::Redo, applied))
            }
            PhysCommand::Interact { interact } => self.interact(interact),
            PhysCommand::Run => {
                self.pot_prior.clear();
                self.journal_push(PhysCommand::Run);
                self.run = RunState::Run;
                Ok(self.run_reply())
            }
            PhysCommand::Stop => {
                self.pot_prior.clear();
                self.journal_push(PhysCommand::Stop);
                self.run = RunState::Stop;
                // In STOP a release takes effect at once.
                let released = self.flush_deferred();
                if self.needs_lower || released {
                    self.recompute();
                }
                Ok(self.run_reply())
            }
            PhysCommand::Reset => {
                self.journal_push(PhysCommand::Reset);
                self.reset();
                Ok(self.run_reply())
            }
            PhysCommand::System { op } => self.system(op),
            PhysCommand::Meter { com, vohm, function } => {
                let circuit = self.doc.circuit();
                let known = |p: &Option<PinRef>| {
                    p.as_ref().map_or(true, |p| circuit.components().get(&p.comp).is_some_and(|c| kind_def(c.kind).pins.iter().any(|d| d.id == p.pin)))
                };
                if !known(com) || !known(vohm) {
                    return Err(Refusal {
                        code: RefusalCode::UnknownPin,
                        ambiguity: None,
                        message_fr: "Broche inconnue pour la pointe du multimètre.".into(),
                        at: None,
                        items: Vec::new(),
                    });
                }
                self.meter.set(com.clone(), vohm.clone(), *function);
                self.journal_push(command.clone());
                self.sample_meter(false);
                self.invalidate();
                Ok(self.run_reply())
            }
        }
    }

    /// See [`Meter::restart`].
    pub fn restart_meter(&mut self) {
        self.meter.restart();
    }

    /// The multimeter as the interface shows it now.
    pub fn meter_value(&self) -> super::MeterValue {
        self.meter.value()
    }

    /// One multimeter reading from the last capture: a frame of RUN (`advance`) or a preview.
    fn sample_meter(&mut self, advance: bool) {
        let Some((vohm, com)) = self.meter.probes() else { return };
        let cap = self.capture.clone();
        let p = Probe { ctx: &cap.ctx, res: &cap.res, blocked: &cap.blocked };
        match self.meter.function {
            MeterFn::VDc => self.meter.volts(advance, p.between(&vohm, &com)),
            MeterFn::Ohm => {
                let ohm = if energized(self.doc.circuit()) {
                    None
                } else {
                    match (p.resolve(&vohm), p.resolve(&com)) {
                        (Ok(a), Ok(b)) => n7rt_solver::unpowered_resistance(&cap.ctx.low.network, a, b),
                        _ => None,
                    }
                };
                self.meter.ohms(ohm);
            }
        }
    }

    /// Mission engine command: the defects change, or a parameter outside the undo history.
    fn system(&mut self, op: &SystemOp) -> Result<CommandReply, Refusal> {
        match op {
            SystemOp::SetParam { comp, name, value } => {
                let applied = self.doc.system_edit(&Edit::SetParam { comp: *comp, name: name.clone(), value: value.clone() })?;
                return Ok(self.committed(PhysCommand::System { op: op.clone() }, applied));
            }
            SystemOp::AttachDefect { a, b, defect } => {
                let circuit = self.doc.circuit();
                let known = |p: &PinRef| circuit.components().get(&p.comp).is_some_and(|c| kind_def(c.kind).pins.iter().any(|d| d.id == p.pin));
                if !known(a) || !known(b) || self.defects.attach(circuit, a, b, *defect).is_err() {
                    return Err(Refusal {
                        code: RefusalCode::UnknownPin,
                        ambiguity: None,
                        message_fr: "Défaut de faisceau invalide.".into(),
                        at: None,
                        items: Vec::new(),
                    });
                }
            }
            SystemOp::DefectOn { wire, defect } => {
                if self.defects.put(self.doc.circuit(), *wire, *defect).is_err() {
                    return Err(Refusal {
                        code: RefusalCode::UnknownWire,
                        ambiguity: None,
                        message_fr: "Défaut de faisceau invalide.".into(),
                        at: None,
                        items: Vec::new(),
                    });
                }
            }
            SystemOp::ClearDefects => self.defects.clear(),
        }
        self.journal_push(PhysCommand::System { op: op.clone() });
        self.needs_lower = true;
        if self.run == RunState::Stop {
            self.recompute();
        }
        Ok(self.run_reply())
    }

    /// Wire objects that carry or carried a hidden defect, and whether one waits for a wire.
    pub fn defects(&self) -> (BTreeSet<WireId>, bool) {
        (self.defects.carriers(), self.defects.pending())
    }

    fn run_reply(&self) -> CommandReply {
        CommandReply::Run(RunReply { run: self.run, sim_time_ns: self.solver.time_ns() })
    }

    fn edit_reply(&self, patch: Patch, nets: Option<Vec<n7rt_circuit::Net>>) -> CommandReply {
        CommandReply::Edit(EditReply {
            rev: self.rev(),
            patch,
            nets,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            dirty: self.dirty(),
        })
    }

    fn committed(&mut self, command: PhysCommand, applied: Applied) -> CommandReply {
        if applied.patch.is_empty() {
            // Not committed by the document: no revision, no journal.
            return self.edit_reply(applied.patch, None);
        }
        self.journal_push(command);
        self.touch();
        for (id, before, after) in &applied.patch.components {
            if before.is_some() && after.is_none() {
                self.deleted_since_load.insert(*id);
                self.pot_prior.remove(id);
                self.unseen_presses.remove(id);
                self.deferred_releases.remove(id);
                self.operating.remove(id);self.rotors.remove(id);self.source_configs.remove(id);
                self.sent.retain(|(ecu,sensor),_|ecu!=id&&sensor!=id);
                self.mechanical.remove(id);self.mechanical_before.remove(id);
                self.prepared_inputs.remove(id);
                self.mission_history.remove(id);
            }
        }
        let netlist = self.doc.circuit().netlist();
        let nets = (netlist != self.doc_netlist).then(|| netlist.nets.clone());
        // both run: a merge moves a defect, an edit gives a pending defect its wire
        if self.defects.follow(&applied.patch) | self.defects.retry(self.doc.circuit()) || applied.electrical_changed || applied.topology_changed {
            self.needs_lower = true;
        } else if !self.needs_lower {
            self.refresh_names(&netlist);
        }
        self.doc_netlist = netlist;
        if self.run == RunState::Stop && self.needs_lower {
            self.recompute();
        }
        self.edit_reply(applied.patch, nets)
    }

    /// Geometry-only edit: the computed values stay, labels and net names follow the document at once.
    fn refresh_names(&mut self, netlist: &Netlist) {
        let lowered = &self.ctx.low.netlist.nets;
        let same = netlist.nets.len() == lowered.len() && netlist.nets.iter().zip(lowered).all(|(a, b)| a.key == b.key && a.pins == b.pins);
        if !same {
            self.needs_lower = true;
            return;
        }
        let ctx = Arc::new(Ctx { circuit: self.doc.circuit().clone(), netlist: netlist.clone(), low: self.ctx.low.clone(), rev: self.doc.rev() });
        if Arc::ptr_eq(&self.capture.ctx.low, &ctx.low) {
            self.capture = Arc::new(Capture { ctx: ctx.clone(), rev: ctx.rev, ..(*self.capture).clone() });
            self.invalidate();
        }
        self.ctx = ctx;
    }

    fn momentary_pressed(&self, comp: CompId) -> bool {
        let circuit = self.doc.circuit();
        circuit.components().get(&comp).is_some_and(|c| {
            c.kind == Kind::PushButton && param_choice(c, "mode") != "latching" && state_bool(circuit, c, "pressed")
        })
    }

    fn interact(&mut self, i: &Interact) -> Result<CommandReply, Refusal> {
        if let Interact::SetButton { comp, pressed } = i {
            let comp = *comp;
            if *pressed {
                // A new press after a deferred release: the release happens first, so every edge is kept.
                if self.deferred_releases.contains(&comp) {
                    self.release_now(comp);
                }
            } else if self.run == RunState::Run && self.unseen_presses.contains(&comp) && self.momentary_pressed(comp) {
                // Validated on a copy; takes effect after the frame that sees the press.
                let mut trial = self.doc.clone();
                let change = trial.interact(i)?;
                self.deferred_releases.insert(comp);
                return Ok(CommandReply::Interact(InteractReply { rev: self.rev(), comp: change.comp, state: change.state }));
            }
        }
        let before = self.doc.circuit().state(interact_comp(i)).cloned();
        let change = self.doc.interact(i)?;
        if before.as_ref() == Some(&change.state) {
            // Nothing changed: nothing to journal or compute.
            return Ok(CommandReply::Interact(InteractReply { rev: self.rev(), comp: change.comp, state: change.state }));
        }
        if let Interact::SetButton { comp, pressed } = i {
            if *pressed {
                self.unseen_presses.insert(*comp);
            } else {
                self.deferred_releases.remove(comp);
            }
        }
        self.journal_interact(i, before.and_then(|s| s.get("position").cloned()));
        self.touch();
        self.needs_lower = true;
        if self.run == RunState::Stop {
            self.recompute();
        }
        Ok(CommandReply::Interact(InteractReply { rev: self.rev(), comp: change.comp, state: change.state }))
    }

    fn release_now(&mut self, comp: CompId) -> bool {
        self.deferred_releases.remove(&comp);
        let i = Interact::SetButton { comp, pressed: false };
        match self.doc.interact(&i) {
            Ok(_) => {
                self.journal_push(PhysCommand::Interact { interact: i });
                self.touch();
                self.needs_lower = true;
                true
            }
            // The button was deleted meanwhile.
            Err(_) => false,
        }
    }

    fn flush_deferred(&mut self) -> bool {
        let pending: Vec<CompId> = std::mem::take(&mut self.deferred_releases).into_iter().collect();
        let mut any = false;
        for comp in pending {
            any |= self.release_now(comp);
        }
        any
    }

    fn journal_push(&mut self, command: PhysCommand) {
        self.journal.push(JournalEntry { frame: self.frame_index, command });
    }

    /// In RUN, several `set_pot` on one potentiometer within a frame: only the last one is journaled, and none when
    /// the wiper is back where it was before the first one (replayed, that entry would be an unchanged state, hence
    /// not journaled). In STOP every change is previewed at once (solver reloaded, islands commuted, ADCs sampled):
    /// each one is journaled, so that the replay goes through the same states.
    fn journal_interact(&mut self, i: &Interact, before: Option<Value>) {
        if let (Interact::SetPot { comp, .. }, RunState::Run) = (i, self.run) {
            let comp = *comp;
            if let Some(k) = self.same_frame_set_pot(comp) {
                self.journal.remove(k);
                let now = self.doc.circuit().state(comp).and_then(|s| s.get("position")).cloned();
                if now.is_some() && self.pot_prior.get(&comp) == now.as_ref() {
                    self.pot_prior.remove(&comp);
                    return;
                }
            } else if let Some(b) = before {
                self.pot_prior.insert(comp, b);
            }
        }
        self.journal_push(PhysCommand::Interact { interact: i.clone() });
    }

    /// Journal index of a `set_pot` of `comp` in the current frame, after the last RUN, STOP or RESET.
    fn same_frame_set_pot(&self, comp: CompId) -> Option<usize> {
        for (k, e) in self.journal.iter().enumerate().rev() {
            if e.frame != self.frame_index || matches!(e.command, PhysCommand::Reset | PhysCommand::Run | PhysCommand::Stop) {
                return None;
            }
            if matches!(&e.command, PhysCommand::Interact { interact: Interact::SetPot { comp: c, .. } } if *c == comp) {
                return Some(k);
            }
        }
        None
    }

    fn reset(&mut self) {
        self.ensure_loaded();
        self.solver.reset();
        self.frame_index = 0;
        self.timers.clear();
        self.adc.clear();
        self.last_modes.clear();
        self.pot_prior.clear();
        self.rotors.clear();self.sent.clear();self.operating.clear();self.prepared_inputs.clear();
        self.mission_history.clear();
        self.mechanical.clear();self.mechanical_before.clear();self.needs_lower=true;
        if self.run == RunState::Stop {
            self.recompute();
        }
    }

    // ------------------------------------------------------------------------------------------- computing

    pub fn step(&mut self) {
        if self.run != RunState::Run {
            return;
        }
        self.ensure_loaded();
        self.prepare_mechanics(true);
        let from=self.solver.time_ns().max(0) as u64;
        self.prepare_mission_inputs(from,from.saturating_add(n7rt_solver::FRAME_NS as u64),true);
        self.ensure_loaded();
        // An island error is in its status: its readings become Unavailable, the others advance.
        let _ = self.solver.step_frame();
        self.frame_index += 1;
        self.pot_prior.clear();
        self.capture(true);
        self.sample_meter(true);
        self.unseen_presses.clear();
        self.flush_deferred();
    }

    /// STOP preview of the current document (contract §5.6): stored state unchanged.
    fn recompute(&mut self) {
        self.prepare_mechanics(false);
        self.ensure_loaded();
        let _ = self.solver.preview();
        let time=self.solver.time_ns().max(0) as u64;
        self.prepare_mission_inputs(time,time,false);
        self.capture(false);
        self.sample_meter(false);
    }

    fn ensure_loaded(&mut self) {
        if !self.needs_lower {
            return;
        }
        self.needs_lower = false;
        let circuit = self.doc.circuit();
        #[allow(unused_mut)]
        let mut low = lower_defective(circuit, &BTreeSet::new(), &self.operating, &self.defects.live(circuit));
        let reborn: BTreeSet<CompId> = self.deleted_since_load.iter().copied().filter(|id| circuit.components().contains_key(id)).collect();
        self.deleted_since_load.clear();
        if !reborn.is_empty() {
            // Let the solver see them gone first: their keys vanish, their state is dropped.
            let _ = self.solver.load(&lower(circuit, &reborn).network);
        }
        #[cfg(test)]
        if std::mem::take(&mut self.corrupt_next_load) {
            if let Some(Element::Resistor { r, .. }) = low.network.elements.first_mut() {
                *r = 0.0;
            }
        }
        match self.solver.load(&low.network) {
            Ok(()) => {
                self.load_error = false;
                // Published fallback modes only belong to supply elements of the accepted network.
                // A supply retained in that network while its gate is off keeps its last mode.
                let supply_keys: BTreeSet<_> = low.network.elements.iter().filter_map(|element| {
                    if let Element::Supply { key, .. } | Element::LimitedVcvs { key, .. } = element { Some(*key) } else { None }
                }).collect();
                self.last_modes.retain(|key, _| supply_keys.contains(key));
                let netlist = low.netlist.clone();
                self.ctx = Arc::new(Ctx { circuit: circuit.clone(), netlist, low: Arc::new(low), rev: self.doc.rev() });
            }
            Err(_) => {
                // A lowering bug: diagnostic internal_error, the previous network stays loaded.
                self.load_error = true;
                if !reborn.is_empty() {
                    let _ = self.solver.load(&self.ctx.low.network);
                }
            }
        }
    }

    fn prepare_mechanics(&mut self,advance:bool) {
        let circuit=self.doc.circuit();
        self.operating.retain(|id,_|circuit.components().contains_key(id));
        self.rotors.retain(|id,_|circuit.components().get(id).is_some_and(|c|c.kind==Kind::Ats682Lsh));
        self.source_configs.retain(|id,_|circuit.components().get(id).is_some_and(|c|c.kind==Kind::EcuSas));
        self.sent.retain(|(ecu,sensor),_|self.source_configs.contains_key(ecu)&&circuit.components().get(sensor).is_some_and(|c|c.kind==Kind::Tle5014));
        self.mechanical.retain(|id,_|circuit.components().get(id).is_some_and(|c|c.kind==Kind::SteeringColumn));
        self.mechanical_before.clear();
        for c in circuit.components().values() {
            if c.kind==Kind::SteeringColumn {
                let target=state_num(circuit,c,"angle_deg").clamp(param_num(c,"min_angle_deg"),param_num(c,"max_angle_deg"));
                let actual=self.mechanical.entry(c.id).or_insert(target);
                // A stopped preview establishes the requested initial position. During RUN,
                // the same target remains rate-limited by the mechanical model below.
                if !advance && self.run==RunState::Stop { *actual=target; }
                self.mechanical_before.insert(c.id,*actual);
                if advance {
                    let max_step=param_num(c,"max_rate_deg_per_s").max(0.0)*n7rt_solver::FRAME_NS as f64/1e9;
                    *actual+=(target-*actual).clamp(-max_step,max_step);
                }
            } else if c.kind==Kind::PressureSwitch {
                let previous=self.operating.get(&c.id).copied().unwrap_or(false);
                let closed=super::bps::contact_closed(circuit,c,previous).unwrap_or(false);
                if self.operating.insert(c.id,closed)!=Some(closed) {self.needs_lower=true;}
            }
        }
    }

    /// Static operating point of a forced output state. Capacitors are open in these L2 point probes;
    /// the live solver's charges, modes and clock are never modified. Its hysteresis state is copied.
    fn operating_point(&self,sensor:CompId,high:bool)->Option<(Ctx,Results)> {
        let mut operating=self.operating.clone(); operating.insert(sensor,high);
        let mut low=lower_defective(self.doc.circuit(),&BTreeSet::new(),&operating,&self.defects.live(self.doc.circuit()));
        let mut indices=BTreeMap::new();
        let elements=low.network.elements.iter().enumerate().filter_map(|(old,e)| {
            if matches!(e,Element::Capacitor{..}) {None} else {Some((old,e.clone()))}
        }).collect::<Vec<_>>();
        low.network.elements=elements.into_iter().enumerate().map(|(new,(old,e))| {indices.insert(old,new);e}).collect();
        for comp in low.comps.values_mut() {
            comp.elems.retain_mut(|e|match indices.get(&e.index) {Some(new)=>{e.index=*new;true},None=>false});
        }
        let mut solver=self.solver.clone(); solver.load(&low.network).ok()?; solver.preview_precise().ok()?;
        let res=Results::extract(&solver,&low); let netlist=low.netlist.clone();
        Some((Ctx {circuit:self.doc.circuit().clone(),netlist,low:Arc::new(low),rev:self.doc.rev()},res))
    }

    fn prepare_mission_inputs(&mut self,from_ns:u64,until_ns:u64,advance:bool) {
        use n7rt_vehicle::{EcuInputs,WsfInputs,WsfLaneInputs,SasInput};
        let circuit=self.doc.circuit().clone(); let none=BTreeMap::new();
        self.prepared_inputs.clear();
        let ecus=circuit.components().values().filter(|c|matches!(c.kind,Kind::EcuWsf|Kind::EcuSas)).cloned().collect::<Vec<_>>();
        let mut wheels=Vec::new();
        for sensor in circuit.components().values().filter(|c|c.kind==Kind::Ats682Lsh) {
            let (Some((lc,lr)),Some((hc,hr)))=(self.operating_point(sensor.id,false),self.operating_point(sensor.id,true)) else {continue};
            let lo=Probe{ctx:&lc,res:&lr,blocked:&none}; let hi=Probe{ctx:&hc,res:&hr,blocked:&none};
            let measurement=super::wsf::LaneMeasurement {low_vdd:raw_voltage(lo.pins_between(sensor.id,"vcc","gnd")),
                high_vdd:raw_voltage(hi.pins_between(sensor.id,"vcc","gnd")),..Default::default()};
            let p=super::wsf::SensorParams::from_component(sensor);
            let mut rotor=self.rotors.get(&sensor.id).cloned().unwrap_or_default();
            let physical=rotor.sample(&p,measurement,super::wsf::wheel_rpm(&circuit,sensor),sensor.id,from_ns,until_ns,self.sensor_seed);
            if advance {
                if self.operating.insert(sensor.id,rotor.current_high)!=Some(rotor.current_high) {self.needs_lower=true;}
                self.rotors.insert(sensor.id,rotor);
            }
            wheels.push((sensor.clone(),lc,lr,hc,hr,physical));
        }
        for ecu in &ecus {
            if ecu.kind==Kind::EcuWsf {
                let mut lanes:[WsfLaneInputs;2]=Default::default();
                for (lane,out) in lanes.iter_mut().enumerate() {
                    let rail=if lane==0 {"rail_l"} else {"rail_r"};
                    // Choose the sensor that electrically changes this sense node, so crossed wiring
                    // transports the crossed wheel's real clock rather than its designator's clock.
                    let selected=wheels.iter().max_by(|a,b| {
                        let effect=|item:&(Component,Ctx,Results,Ctx,Results,WsfLaneInputs)| {
                            let lo=Probe{ctx:&item.1,res:&item.2,blocked:&none};let hi=Probe{ctx:&item.3,res:&item.4,blocked:&none};
                            let pin=if lane==0 {"ws_l_in"}else{"ws_r_in"};
                            match (raw_voltage(lo.pins_between(ecu.id,pin,"gnd")),raw_voltage(hi.pins_between(ecu.id,pin,"gnd"))) {
                                // A sensor's higher current raises its own sense voltage. A
                                // neighbour only lowers it through the shared LV impedance;
                                // that indirect effect must never transport the neighbour's clock.
                                (Some(l),Some(h))=>(h-l).max(0.0),_=>0.0}
                        };
                        effect(a).total_cmp(&effect(b)).then_with(|| {
                            let side=if lane==0 {"left"}else{"right"};
                            (param_choice(&a.0,"wheel_side")==side).cmp(&(param_choice(&b.0,"wheel_side")==side))
                        })
                    });
                    if let Some((sensor,lc,lr,hc,hr,physical))=selected {
                        let lo=Probe{ctx:lc,res:lr,blocked:&none};let hi=Probe{ctx:hc,res:hr,blocked:&none};
                        let m=super::wsf::measure(&lo,&hi,ecu.id,sensor.id,lane,rail);
                        *out=physical.clone();out.low_sup_v=m.low_supply_v;out.high_sup_v=m.high_supply_v;
                        out.low_v_rs=m.low_v_rs;out.high_v_rs=m.high_v_rs;
                        out.sup_v=if self.operating.get(&sensor.id).copied().unwrap_or(true) {m.high_supply_v}else{m.low_supply_v};
                        out.limited=(out.present_low&&m.low_limited)||(out.present_high&&m.high_limited);
                    } else {
                        let res=Results::extract(&self.solver,&self.ctx.low);let p=Probe{ctx:&self.ctx,res:&res,blocked:&none};
                        let (sup,input)=if lane==0 {("ws_l_sup","ws_l_in")}else{("ws_r_sup","ws_r_in")};
                        out.low_sup_v=raw_voltage(p.pins_between(ecu.id,sup,"gnd"));out.high_sup_v=out.low_sup_v;
                        out.low_v_rs=raw_voltage(p.pins_between(ecu.id,input,"gnd"));out.high_v_rs=out.low_v_rs;
                        out.present_high=true;
                    }
                    if !advance {out.edges.clear();}
                }
                self.prepared_inputs.insert(ecu.id,EcuInputs{wsf:Some(WsfInputs{lv:None,lanes}),..Default::default()});
            } else if let Some(config)=self.source_configs.get(&ecu.id).cloned() {
                let mut frames=Vec::new();
                for sensor in circuit.components().values().filter(|c|c.kind==Kind::Tle5014) {
                    let (Some((lc,lr)),Some((hc,hr)))=(self.operating_point(sensor.id,false),self.operating_point(sensor.id,true)) else {continue};
                    let lo=Probe{ctx:&lc,res:&lr,blocked:&none};let hi=Probe{ctx:&hc,res:&hr,blocked:&none};
                    let p=sas_sensor_config(sensor,ecu,&config);
                    let fingerprint=serde_json::to_string(&p).unwrap_or_default();
                    let key=(ecu.id,sensor.id);
                    if self.sent.get(&key).is_none_or(|(old,_)|*old!=fingerprint) {
                        if let Ok(generator)=super::sas::SentGenerator::new(p,&config) {self.sent.insert(key,(fingerprint,generator));}
                        else {self.sent.remove(&key);continue;}
                    }
                    let world=u32::try_from(param_int(sensor,"world_comp_id")).ok().map(CompId);
                    let angle=world.and_then(|id|self.mechanical.get(&id).copied()).unwrap_or(f64::NAN);
                    let angle_before=world.and_then(|id|self.mechanical_before.get(&id).copied()).unwrap_or(angle);
                    let actual=Results::extract(&self.solver,&self.ctx.low);let raw=Probe{ctx:&self.ctx,res:&actual,blocked:&none};
                    let m=super::sas::SasMeasurement{vdd_v:raw_voltage(lo.pins_between(sensor.id,"vdd","gnd")),
                        out_v:raw_voltage(raw.pins_between(sensor.id,"out","gnd")).or_else(||raw_voltage(hi.pins_between(sensor.id,"out","gnd"))),
                        ecu_low_v:raw_voltage(lo.internal_between(ecu.id,"sent_rx","gnd")),
                        ecu_high_v:raw_voltage(hi.internal_between(ecu.id,"sent_rx","gnd")),
                        angle_deg:angle,angle_start_deg:angle_before};
                    if advance {if let Some((_,generator))=self.sent.get_mut(&key) {frames.extend(generator.advance(from_ns/1000,until_ns/1000,m));}}
                }
                frames.sort_by_key(|f|(f.completed_us,f.started_us));
                self.prepared_inputs.insert(ecu.id,EcuInputs{sas:Some(SasInput{lv:None,supply_adc:None,frames}),..Default::default()});
            }
        }
    }

    fn capture(&mut self, advance: bool) {
        let res = Arc::new(Results::extract(&self.solver, &self.ctx.low));
        let none = BTreeMap::new();
        let raw = Probe { ctx: &self.ctx, res: &res, blocked: &none };
        let mut xcvr = BTreeMap::new();
        for c in self.ctx.circuit.components().values().filter(|c| c.kind == Kind::CanTransceiver) {
            let mut timer = self.timers.get(&c.id).copied().unwrap_or_default();
            xcvr.insert(c.id, eval_xcvr(&raw, c, &mut timer, advance));
            if advance {
                self.timers.insert(c.id, timer);
            }
        }
        if advance {
            self.timers.retain(|id, _| xcvr.contains_key(id));
        }
        let blocked = blocked_nodes(&raw, &xcvr);
        let probe = Probe { ctx: &self.ctx, res: &res, blocked: &blocked };
        sample_adcs(&probe, advance.then_some(self.frame_index), res.time_ns, &mut self.adc);
        for (e, el) in self.ctx.low.network.elements.iter().enumerate() {
            if let (Element::Supply { key, .. } | Element::LimitedVcvs { key, .. }, Some(Some(m))) = (el, res.modes.get(e)) {
                self.last_modes.insert(*key, *m);
            }
        }
        let mut inputs=self.prepared_inputs.clone();
        for c in self.ctx.circuit.components().values().filter(|c|matches!(c.kind,Kind::EcuApps|Kind::EcuWsf|Kind::EcuSas|Kind::EcuBps)) {
            let entry=inputs.entry(c.id).or_default();
            entry.lv=raw_voltage(probe.pins_between(c.id,"lv_in","gnd"));
            let channels:&[&str]=match c.kind {Kind::EcuApps=>&ECU_APPS_CHANNELS,Kind::EcuBps=>&["sig_f","sig_r","sup5v"],_=>&[]};
            for (i,ch) in channels.iter().enumerate() {entry.adc[i]=self.adc.get(&(c.id,*ch)).and_then(adc_code);}
            if c.kind==Kind::EcuBps {entry.contact_v=raw_voltage(probe.pins_between(c.id,"sw_in","gnd"));}
            if let Some(wsf)=entry.wsf.as_mut() {wsf.lv=entry.lv;}
            if let Some(sas)=entry.sas.as_mut() {sas.lv=entry.lv;sas.supply_adc=self.adc.get(&(c.id,"sup5v")).and_then(adc_code);}
        }
        self.mission_history.retain(|id,_|inputs.contains_key(id));
        let time=res.time_ns.max(0) as u64;
        for (ecu,input) in &inputs {
            let history=self.mission_history.entry(*ecu).or_default();
            if history.back().is_some_and(|(t,_)|*t>time) {history.clear();}
            if history.back().is_some_and(|(t,_)|*t==time) {history.pop_back();}
            history.push_back((time,Arc::new(input.clone())));
            // A source polling late must report missing physical time rather than replay a new sample.
            while history.len()>256 {history.pop_front();}
        }
        self.capture = Arc::new(Capture {
            ctx: self.ctx.clone(),
            res,
            xcvr: Arc::new(xcvr),
            blocked: Arc::new(blocked),
            adc: Arc::new(self.adc.clone()),
            mission_inputs:Arc::new(inputs),
            mission_history:Arc::new(self.mission_history.clone()),
            mechanical:Arc::new(self.mechanical.clone()),
            last_modes: Arc::new(self.last_modes.clone()),
            load_error: self.load_error,
            rev: self.ctx.rev,
        });
        self.invalidate();
    }

    // -------------------------------------------------------------------------------------------- outputs

    pub fn frame(&self) -> Arc<PhysFrame> {
        if let Some(f) = self.frame_cache.borrow().as_ref() {
            return f.clone();
        }
        let inner = Frame::build(self.capture.clone(), self.capture.rev + self.rev_offset, self.disclosure, &self.notices);
        let f = Arc::new(PhysFrame { inner });
        *self.frame_cache.borrow_mut() = Some(f.clone());
        f
    }

    pub fn values(&self, seq: u64, disclosure: Disclosure, engine: EngineStats) -> ValuesDto {
        let frame = self.frame();
        let f = &frame.inner;
        let p = f.probe();
        let mission = disclosure == Disclosure::Mission;
        let circuit = &f.cap.ctx.circuit;
        let doc = self.doc.circuit();
        let nets = if mission {
            Vec::new()
        } else {
            f.cap.ctx.netlist.nets.iter().map(|n| NetValue { key: n.key.clone(), name: n.name.clone(), voltage: p.voltage_at(&n.key) }).collect()
        };
        let comps = circuit
            .components()
            .values()
            .map(|c| {
                // Interaction states are the document's: a reply never goes back to an older position.
                let state = doc.state(c.id).or_else(|| circuit.state(c.id)).cloned().unwrap_or_default();
                let op = f.op_state(c.id).unwrap_or(OpState::Passive);
                if mission {
                    CompValues { id: c.id, state, op: mission_op(op), pins: Vec::new(), measures: Vec::new() }
                } else {
                    CompValues { id: c.id, state, op, pins: pin_values(&p, c), measures: measures(f, &p, c) }
                }
            })
            .collect();
        ValuesDto {
            seq,
            doc_id: self.doc_id,
            rev: f.rev,
            sim_time_ns: f.cap.res.time_ns,
            run: self.run,
            preview: f.cap.res.preview,
            disclosure,
            nets,
            comps,
            diagnostics: if mission { Vec::new() } else { f.diagnostics.clone() },
            meter: self.meter.value(),
            engine,
        }
    }

    pub fn snapshot(&self, name: &str) -> SnapshotDto {
        let c = self.doc.circuit();
        SnapshotDto {
            doc_id: self.doc_id,
            rev: self.rev(),
            name: name.to_string(),
            dirty: self.dirty(),
            components: c.components().values().cloned().collect(),
            wires: c.wires().values().cloned().collect(),
            states: c.components().keys().filter_map(|id| c.state(*id).map(|s| (*id, s.clone()))).collect(),
            nets: self.doc_netlist.nets.clone(),
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            run: self.run,
            sim_time_ns: self.solver.time_ns(),
            disclosure: self.disclosure,
        }
    }

    /// FNV-1a 64 of the canonical circuit text, the interaction states, the run state, the frame index and the
    /// solver state digest (contract §8.2).
    pub fn digest(&self) -> u64 {
        let circuit = self.doc.circuit();
        let mut h = Fnv::new();
        h.str(&persist::save(circuit));
        for id in circuit.components().keys() {
            if let Some(state) = circuit.state(*id) {
                h.u32(id.0);
                h.u64(state.len() as u64);
                for (k, v) in state {
                    h.str(k);
                    h.value(v);
                }
            }
        }
        h.u8(match self.run {
            RunState::Run => 1,
            RunState::Stop => 0,
        });
        h.u64(self.frame_index);
        h.u64(self.solver.state_digest());
        if !self.defects.is_empty() {
            self.defects.digest(&mut h);
        }
        h.finish()
    }
}

/// Replays a journal from `initial`: entries are applied at their frame index, then frames run up to `frames`
/// (the frame index reached since the last RESET of the journal).
pub(crate) fn replay(initial: &Circuit, journal: &[JournalEntry], frames: u64) -> Core {
    let mut core = Core::new(initial.clone());
    for e in journal {
        while core.run == RunState::Run && core.frame_index < e.frame {
            core.step();
        }
        // The journal holds committed commands only; a refusal here would be a divergence, seen in the digest.
        let _ = core.apply(&e.command);
    }
    while core.run == RunState::Run && core.frame_index < frames {
        core.step();
    }
    core
}

fn interact_comp(i: &Interact) -> CompId {
    match i {
        Interact::SetPot { comp, .. } | Interact::SetButton { comp, .. } | Interact::SetSwitch { comp, .. } | Interact::SetPower { comp, .. } | Interact::SetWorld {comp,..} => *comp,
    }
}

fn raw_voltage(r:Reading<f64>)->Option<f64> {match r {Reading::Valid{v} if v.is_finite()=>Some(v),_=>None}}
fn adc_code(r:&Reading<AdcSample>)->Option<u32> {match r {Reading::Valid{v}|Reading::Saturated{v}=>Some(v.code),_=>None}}
fn sas_sensor_config(sensor:&Component,ecu:&Component,source:&n7rt_vehicle::SasConfig)->super::sas::SasSensorConfig {
    super::sas::SasSensorConfig {por_on_v:param_num(sensor,"reset_on"),por_off_v:param_num(sensor,"reset_off"),
        overvoltage_v:param_num(sensor,"overvoltage_v"),out_overvoltage_margin_v:param_num(sensor,"out_overvoltage_margin_v"),
        boot_us:(param_num(sensor,"boot_ms")*1000.0).round().max(0.0) as u64,
        phase_us:param_num(sensor,"phase_us").round().max(0.0) as u64,tick_us:param_num(sensor,"ut_us"),
        data_zero:param_int(sensor,"data_offset"),mounting_direction:param_int(sensor,"mounting_direction"),
        turn_deg:source.turn_degrees as f64,static_error_deg:param_num(sensor,"error_deg"),
        fault_angle_deg:param_num(sensor,"fault_angle_deg"),sensor_error:param_bool(sensor,"sensor_error"),
        receiver_vil_v:0.3*param_num(ecu,"logic_v"),receiver_vih_v:0.7*param_num(ecu,"logic_v"),
        max_advance_us:n7rt_solver::FRAME_NS as u64/1000}
}

/// Sample-and-hold of the ADC channels. At the end of a frame (`frame` = its index), the channels due at this index;
/// in a STOP preview (`None`), every channel at the frozen instant, so that the code at 0+ of an edit or an
/// interaction is shown (contract §9.1: ADC1 = 2582 at 0+ after the wiper moves) and held until the next due frame.
fn sample_adcs(p: &Probe, frame: Option<u64>, t_ns: i64, holds: &mut AdcHolds) {
    let circuit = &p.ctx.circuit;
    let due = |rate: i64| frame.map_or(true, |f| f % sample_period(rate) == 0);
    for c in circuit.components().values() {
        match c.kind {
            Kind::AdcInput => {
                if due(param_int(c, "sample_rate")) {
                    let bits = param_int(c, "bits").clamp(1, 16) as u32;
                    let v = p.pins_between(c.id, "in", "ref");
                    holds.insert((c.id, "in"), sample(&v, param_num(c, "vref"), bits, t_ns));
                }
            }
            Kind::EcuStm32 => {
                if due(param_int(c, "adc_sample_rate")) {
                    let bits = param_int(c, "adc_bits").clamp(1, 16) as u32;
                    for ch in ["a0", "a1"] {
                        let s = if !ecu_powered(circuit, c) {
                            unavailable("ecu_unpowered")
                        } else {
                            // Ratiometric: Vref = V(3V3) − V(GND) at the sampling instant.
                            match p.pins_between(c.id, "v3v3", "gnd") {
                                Reading::Valid { v: vref } => sample(&p.pins_between(c.id, ch, "gnd"), vref, bits, t_ns),
                                other => recast(&other),
                            }
                        };
                        holds.insert((c.id, ch), s);
                    }
                }
            }
            Kind::EcuApps | Kind::EcuBps | Kind::EcuSas => {
                // Every frame (1 kHz); in reset the converter does not run.
                let on = p.part_gate(c.id, None);
                let channels:&[&'static str]=match c.kind {Kind::EcuApps=>&ECU_APPS_CHANNELS,Kind::EcuBps=>&["sig_f","sig_r","sup5v"],_=>&["sup5v"]};
                let bits=if c.kind==Kind::EcuApps {ECU_APPS_ADC_BITS}else{param_int(c,"adc_bits").clamp(1,16) as u32};
                let reference=if c.kind==Kind::EcuApps {ECU_APPS_ADC_VREF}else{param_num(c,"adc_vref")};
                for &ch in channels {
                    let s = if on == Some(false) {
                        unavailable("ecu_unpowered")
                    } else {
                        sample(&p.internal_between(c.id, ch, "gnd"), reference, bits, t_ns)
                    };
                    holds.insert((c.id, ch), s);
                }
            }
            _ => {}
        }
    }
    holds.retain(|(id, _), _| circuit.components().contains_key(id));
}

/// What the real part shows in mission mode: contact, wiper, CC of a bench supply, board powered.
fn mission_op(op: OpState) -> OpState {
    match op {
        OpState::Supply { output: SupplyOutput::Rev } => OpState::Supply { output: SupplyOutput::Cv },
        OpState::Transceiver { .. } => OpState::Passive,
        other => other,
    }
}

fn pin_values(p: &Probe, c: &Component) -> Vec<PinValue> {
    kind_def(c.kind)
        .pins
        .iter()
        .map(|d| {
            let pin = PinRef { comp: c.id, pin: d.id.to_string() };
            PinValue { pin: d.id.to_string(), voltage: p.voltage_at(&pin), current: p.pin_current(&pin) }
        })
        .collect()
}

fn measure(name: &str, label_fr: &str, unit: &str, reading: Reading<f64>) -> Measure {
    Measure { name: name.to_string(), label_fr: label_fr.to_string(), unit: unit.to_string(), reading }
}

fn negated(r: Reading<f64>) -> Reading<f64> {
    match r {
        Reading::Valid { v } => Reading::Valid { v: -v },
        other => other,
    }
}

fn adc_measures(out: &mut Vec<Measure>, s: Reading<AdcSample>, names: [&str; 2], labels: [&str; 2]) {
    let (v, code) = match &s {
        Reading::Valid { v: x } => (Reading::Valid { v: x.volts }, Reading::Valid { v: f64::from(x.code) }),
        // The voltage itself is a valid measurement; the code is the clamped one.
        Reading::Saturated { v: x } => (Reading::Valid { v: x.volts }, Reading::Saturated { v: f64::from(x.code) }),
        other => (recast(other), recast(other)),
    };
    out.push(measure(names[0], labels[0], "V", v));
    out.push(measure(names[1], labels[1], "", code));
}

fn measures(f: &Frame, p: &Probe, c: &Component) -> Vec<Measure> {
    let id = c.id;
    let pin = |s: &str| PinRef { comp: id, pin: s.to_string() };
    let elem = |name: &str| -> Reading<f64> {
        match p.ctx.low.comps.get(&id).and_then(|l| l.elem(name)) {
            Some(e) => match p.elem_current(e) {
                Ok(i) => finite(i),
                Err(r) => r,
            },
            None => unavailable("ecu_unpowered"),
        }
    };
    let mut v = Vec::new();
    match c.kind {
        Kind::DcSupply => {
            v.push(measure("v_out", "Tension de sortie", "V", p.pins_between(id, "pos", "neg")));
            v.push(measure("i_out", "Courant débité", "A", negated(p.pin_current(&pin("pos")))));
            v.push(measure("power", "Puissance absorbée", "W", p.power(id)));
        }
        Kind::Resistor | Kind::Termination | Kind::Switch | Kind::PushButton | Kind::PressureSwitch => {
            v.push(measure("v", "Tension aux bornes", "V", p.pins_between(id, "p1", "p2")));
            v.push(measure("power", "Puissance dissipée", "W", p.power(id)));
        }
        Kind::Capacitor => {
            v.push(measure("v_c", "Tension aux bornes", "V", p.pins_between(id, "p1", "p2")));
            v.push(measure("i_c", "Courant entrant par la broche 1", "A", p.pin_current(&pin("p1"))));
            v.push(measure("power", "Puissance absorbée", "W", p.power(id)));
        }
        Kind::Potentiometer => {
            v.push(measure("i_aw", "Courant du segment A\u{2013}W", "A", elem("r_aw")));
            v.push(measure("i_wb", "Courant du segment W\u{2013}B", "A", elem("r_wb")));
            v.push(measure("power", "Puissance dissipée", "W", p.power(id)));
        }
        Kind::AdcInput => {
            adc_measures(&mut v, f.adc(id, "in"), ["v_in", "code"], ["Tension échantillonnée", "Code"]);
        }
        Kind::EcuStm32 => {
            adc_measures(&mut v, f.adc(id, "a0"), ["a0_v", "a0_code"], ["Tension échantillonnée A0", "Code A0"]);
            adc_measures(&mut v, f.adc(id, "a1"), ["a1_v", "a1_code"], ["Tension échantillonnée A1", "Code A1"]);
            v.push(measure("i_5v", "Courant du rail 5V", "A", elem("rail_5v")));
            v.push(measure("i_3v3", "Courant du rail 3V3", "A", elem("rail_3v3")));
            v.push(measure("power", "Puissance absorbée", "W", p.power(id)));
        }
        Kind::CanTransceiver => {
            v.push(measure("u_dd", "VDD \u{2212} VSS", "V", p.pins_between(id, "vdd", "vss")));
            v.push(measure("v_diff", "CANH \u{2212} CANL", "V", p.pins_between(id, "canh", "canl")));
            v.push(measure("i_dd", "Courant absorbé par VDD", "A", p.pin_current(&pin("vdd"))));
            v.push(measure("power", "Puissance absorbée", "W", p.power(id)));
        }
        Kind::CanBus => {
            v.push(measure("v_diff", "CANH \u{2212} CANL", "V", p.pins_between(id, "canh", "canl")));
            v.push(measure("power", "Puissance absorbée", "W", p.power(id)));
        }
        // A black box: what its connector shows, nothing inside (conditioning, ADC) is published.
        Kind::EcuApps | Kind::EcuWsf | Kind::EcuSas | Kind::EcuBps => {
            v.push(measure("u_lv", "LV_IN \u{2212} GND", "V", p.pins_between(id, "lv_in", "gnd")));
            v.push(measure("v_diff", "CAN_H \u{2212} CAN_L", "V", p.pins_between(id, "can_h", "can_l")));
        }
        Kind::Mlx90377 => {
            v.push(measure("u_dd1", "VDD1 \u{2212} VSS1", "V", p.pins_between(id, "vdd1", "vss1")));
            v.push(measure("u_out1", "OUT1 \u{2212} VSS1", "V", p.pins_between(id, "out1", "vss1")));
            v.push(measure("u_dd2", "VDD2 \u{2212} VSS2", "V", p.pins_between(id, "vdd2", "vss2")));
            v.push(measure("u_out2", "OUT2 \u{2212} VSS2", "V", p.pins_between(id, "out2", "vss2")));
        }
        Kind::Ats682Lsh => {
            v.push(measure("u_dd","VCC - GND","V",p.pins_between(id,"vcc","gnd")));
            v.push(measure("i_dd","Courant entrant par VCC","A",p.pin_current(&pin("vcc"))));
        }
        Kind::Tle5014 => {
            v.push(measure("u_dd","VDD - GND","V",p.pins_between(id,"vdd","gnd")));
            v.push(measure("u_out","OUT - GND","V",p.pins_between(id,"out","gnd")));
        }
        Kind::Pss260 => {
            v.push(measure("u_dd","US - GND","V",p.pins_between(id,"us","gnd")));
            v.push(measure("u_out","SIG - GND","V",p.pins_between(id,"sig","gnd")));
        }
        Kind::Ground | Kind::Junction | Kind::FrontAxle | Kind::SteeringColumn | Kind::BrakeHydraulics => {}
    }
    v
}

#[cfg(test)]
#[path = "../../tests/retention/kernel.rs"]
mod retention_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use n7rt_circuit::{Pt, Rot, Value, ValueMap};

    fn add(core: &mut Core, kind: Kind, x: i32, params: &[(&str, Value)]) -> CompId {
        let params: ValueMap = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let edit = Edit::AddComponent { kind, pos: Pt { x, y: 0 }, rot: Rot::R0, params, label: None };
        match core.apply(&PhysCommand::Edit { base_rev: core.rev(), edit }).expect("placed") {
            CommandReply::Edit(r) => r.patch.components.iter().find(|(_, b, a)| b.is_none() && a.is_some()).map(|(id, _, _)| *id).expect("new id"),
            _ => panic!("edit reply expected"),
        }
    }

    #[test]
    fn refused_lowering_keeps_previous_network() {
        let mut core = Core::new(Circuit::new("t"));
        let r = add(&mut core, Kind::Resistor, 0, &[]);
        assert!(!core.load_error);
        let codes = |core: &Core| core.frame().inner.diagnostics.iter().map(|d| d.code.clone()).collect::<Vec<_>>();
        assert!(!codes(&core).contains(&"internal_error".to_string()));
        core.corrupt_next_load = true;
        core.apply(&PhysCommand::Edit {
            base_rev: core.rev(),
            edit: Edit::SetParam { comp: r, name: "resistance".into(), value: Value::Num(470.0) },
        })
        .expect("committed in the document");
        assert!(core.load_error);
        assert!(codes(&core).contains(&"internal_error".to_string()));
        // The previous network is still the solver's: the old value is what it computes with.
        assert!(matches!(core.ctx.low.network.elements[0], Element::Resistor { r, .. } if r == 1e4));
        // The next lowering succeeds and clears the diagnostic.
        core.apply(&PhysCommand::Edit {
            base_rev: core.rev(),
            edit: Edit::SetParam { comp: r, name: "resistance".into(), value: Value::Num(220.0) },
        })
        .expect("committed");
        assert!(!core.load_error);
        assert!(!codes(&core).contains(&"internal_error".to_string()));
    }

    #[test]
    fn revision_offset_and_stale_refusal() {
        let mut core = Core::new(Circuit::new("t"));
        core.set_rev_offset(40);
        let rev = core.rev();
        assert_eq!(rev, core.document().rev() + 40);
        let edit = Edit::AddComponent { kind: Kind::Resistor, pos: Pt { x: 0, y: 0 }, rot: Rot::R0, params: ValueMap::new(), label: None };
        let refused = core.apply(&PhysCommand::Edit { base_rev: rev - 1, edit: edit.clone() }).expect_err("stale");
        assert_eq!(refused.code, RefusalCode::StaleRevision);
        assert!(core.journal().is_empty());
        let reply = core.apply(&PhysCommand::Edit { base_rev: rev, edit: edit.clone() }).expect("current");
        let CommandReply::Edit(r) = reply else { panic!("edit reply") };
        assert!(r.rev > rev);
        // Journaled with the raw document revision, so that replay (offset 0) accepts it.
        assert_eq!(core.journal()[0].command, PhysCommand::Edit { base_rev: rev - 40, edit });
        let replayed = replay(&Circuit::new("t"), core.journal(), core.frame_index());
        assert_eq!(replayed.digest(), core.digest());
    }
}

//! A virtual bench: a copy of a circuit in its own physics core, with the mission's source ECU configured, driven
//! millisecond by millisecond together with a vehicle, exactly as the live session drives the student's bench. It
//! never touches the live document. Used by the instructor edition to render hidden sequences on the reference bench
//! (anomalies switched on and off at chosen instants), and by the student application to compute the expected value
//! of a key measure, with and without the anomaly, on the circuit the student had when recording it.

use std::collections::BTreeMap;

use n7rt_circuit::{Circuit, CompId, Interact, PinRef, Value};
use n7rt_vehicle::{BusFrame, EcuInputs, Pack, Vehicle};

use crate::phys::{CircuitProbe, PhysCommand, PhysCore, Reading};
use crate::workflow::incident::PinName;

pub struct VirtualBench {
    core: PhysCore,
    pub ecu: CompId,
    pub sensor: CompId,
    channels: Vec<String>,
    ranges: BTreeMap<String, [f64; 2]>,
}

fn bad(what: &str) -> String {
    format!("banc virtuel : {what}")
}

/// The sensor command quantities of a mission: the pot position for a pack without world ranges, else its world
/// quantities.
pub fn quantities(pack: &Pack) -> Vec<String> {
    if pack.world_ranges().is_empty() {
        vec!["position".to_owned()]
    } else {
        pack.world_ranges().keys().cloned().collect()
    }
}

impl VirtualBench {
    /// The bench `circuit` for `pack` (its designators and source configuration), in RUN.
    pub fn new(circuit: Circuit, pack: &Pack, sensor_seed: u64) -> Result<Self, String> {
        let find = |label: &str| circuit.components().values().find(|c| c.label == label).map(|c| c.id);
        let ecu = find(pack.bench_ecu()).ok_or_else(|| bad("ECU absent"))?;
        let sensor = find(pack.bench_sensor()).ok_or_else(|| bad("capteur absent"))?;
        let mut core = PhysCore::new(circuit);
        core.configure_source(ecu, pack)?;
        core.set_sensor_seed(sensor_seed);
        core.apply(&PhysCommand::Run).map_err(|_| bad("marche"))?;
        Ok(VirtualBench { core, ecu, sensor, channels: pack.adc_channels().to_vec(), ranges: pack.world_ranges().clone() })
    }

    pub fn core(&self) -> &PhysCore {
        &self.core
    }

    pub fn apply(&mut self, command: &PhysCommand) -> Result<(), String> {
        self.core.apply(command).map(|_| ()).map_err(|r| bad(&r.message_fr))
    }

    /// The pin of the circuit named `(designator, pin)`.
    pub fn pin(&self, p: &PinName) -> Option<PinRef> {
        let c = self.core.document().circuit().components().values().find(|c| c.label == p.0)?;
        Some(PinRef { comp: c.id, pin: p.1.clone() })
    }

    pub fn component(&self, designator: &str) -> Option<CompId> {
        self.core.document().circuit().components().values().find(|c| c.label == designator).map(|c| c.id)
    }

    /// The sensor's commanded quantities now (« position » or the world quantities).
    pub fn sensor_state(&self) -> BTreeMap<String, f64> {
        let circuit = self.core.document().circuit();
        let state = circuit.state(self.sensor).cloned().unwrap_or_default();
        let keys: Vec<String> = if self.ranges.is_empty() { vec!["position".to_owned()] } else { self.ranges.keys().cloned().collect() };
        keys.into_iter().filter_map(|k| match state.get(&k) {
            Some(Value::Num(v)) => Some((k, *v)),
            _ => None,
        }).collect()
    }

    /// Commands the sensor (`position` for a pot, world quantities otherwise).
    pub fn set_sensor(&mut self, values: &BTreeMap<String, f64>) -> Result<(), String> {
        for (quantity, value) in values {
            let interact = if quantity == "position" {
                Interact::SetPot { comp: self.sensor, position: *value }
            } else {
                Interact::SetWorld { comp: self.sensor, quantity: quantity.clone(), value: *value }
            };
            self.apply(&PhysCommand::Interact { interact })?;
        }
        Ok(())
    }

    /// `ms` frames of physics alone.
    pub fn settle(&mut self, ms: u64) {
        for _ in 0..ms {
            self.core.step();
        }
    }

    /// Mean of V(a) − V(b) over `frames` frames, like the multimeter; `None` when a sample is not valid.
    pub fn mean_voltage(&mut self, a: &PinRef, b: &PinRef, frames: u64) -> Option<f64> {
        let mut sum = 0.0;
        for _ in 0..frames.max(1) {
            self.core.step();
            match self.core.frame().voltage_between_pins(a, b) {
                Reading::Valid { v } if v.is_finite() => sum += v,
                _ => return None,
            }
        }
        Some(sum / frames.max(1) as f64)
    }

    /// The ECU inputs of the live sampling (lowered source inputs when the core provides them, else LV and ADC codes).
    pub fn inputs(&self) -> EcuInputs {
        let frame = self.core.frame();
        if let Some(input) = frame.mission_source_inputs(self.ecu) {
            return input;
        }
        let pin = |p: &str| PinRef { comp: self.ecu, pin: p.to_owned() };
        let lv = match frame.voltage_between_pins(&pin("lv_in"), &pin("gnd")) {
            Reading::Valid { v } => Some(v),
            _ => None,
        };
        let mut adc = [None; 4];
        for (value, channel) in adc.iter_mut().zip(&self.channels) {
            *value = match frame.adc(self.ecu, channel) {
                Reading::Valid { v } => Some(v.code),
                _ => None,
            };
        }
        EcuInputs { lv, adc, ..Default::default() }
    }

    /// Runs the vehicle up to `until_us` with this bench as its sensor side: for every millisecond, `each(ms, bench)`
    /// may command the bench (sensor, system operations), then one physics frame runs (from ms 1) and the ECU
    /// samples the bench. Err when a command of `each` is refused.
    pub fn drive(
        &mut self,
        vehicle: &mut Vehicle,
        until_us: u64,
        each: &mut dyn FnMut(u64, &mut VirtualBench) -> Result<(), String>,
    ) -> Result<Vec<BusFrame>, String> {
        let mut failure = None;
        let frames = vehicle.advance(until_us, &mut |ms| {
            if failure.is_none() {
                if let Err(e) = each(ms, self) {
                    failure = Some(e);
                }
            }
            if ms > 0 {
                self.core.step();
            }
            self.inputs()
        });
        match failure {
            Some(e) => Err(e),
            None => Ok(frames),
        }
    }
}

//! Frame-level SENT transport for a physical angle sensor. No mission value is compiled here.
//! The network is solved with the sensor output low and high before constructing measurements.

use n7rt_vehicle::{SasConfig, SentConfig, SentFrame};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SasSensorConfig {
    pub por_on_v: f64,
    pub por_off_v: f64,
    pub overvoltage_v: f64,
    pub out_overvoltage_margin_v: f64,
    pub boot_us: u64,
    pub phase_us: u64,
    pub tick_us: f64,
    pub data_zero: i64,
    pub mounting_direction: i64,
    pub turn_deg: f64,
    pub static_error_deg: f64,
    pub fault_angle_deg: f64,
    pub sensor_error: bool,
    pub receiver_vil_v: f64,
    pub receiver_vih_v: f64,
    pub max_advance_us: u64,
}

#[cfg(all(test, n7rt_lab_oracle_tests))]
mod tests {
    include!(concat!(env!("N7RT_LAB_ORACLE_TESTS"), "/phys_sas.rs"));
}

impl SasSensorConfig {
    pub(crate) fn validate(&self, source: &SasConfig) -> Result<(), String> {
        source.validate()?;
        if [self.por_on_v, self.por_off_v, self.overvoltage_v, self.out_overvoltage_margin_v,
            self.tick_us, self.turn_deg, self.static_error_deg, self.fault_angle_deg,
            self.receiver_vil_v, self.receiver_vih_v].iter().any(|v| !v.is_finite())
            || self.por_off_v < 0.0 || self.por_on_v < self.por_off_v
            || self.overvoltage_v <= self.por_on_v || self.out_overvoltage_margin_v < 0.0
            || self.tick_us < source.sent.tick_min_us || self.tick_us > source.sent.tick_max_us
            || self.turn_deg <= 0.0 || ![-1, 1].contains(&self.mounting_direction)
            || self.receiver_vil_v >= self.receiver_vih_v || self.max_advance_us == 0
            || (self.max_advance_us as f64 / (source.sent.frame_ticks as f64 * self.tick_us)).ceil() + 1.0
                > source.history_capacity as f64
            || self.data_zero < source.sent.data_min as i64 || self.data_zero > source.sent.data_max as i64
        {
            return Err("modèle capteur SENT : paramètres invalides".into());
        }
        Ok(())
    }
}

/// Potentials at the sensor and at the ECU, all measured against their actual grounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SasMeasurement {
    pub vdd_v: Option<f64>,
    pub out_v: Option<f64>,
    pub ecu_low_v: Option<f64>,
    pub ecu_high_v: Option<f64>,
    pub angle_start_deg: f64,
    pub angle_deg: f64,
}

impl SasMeasurement {
    fn transmits(&self, p: &SasSensorConfig) -> bool {
        matches!((self.ecu_low_v, self.ecu_high_v), (Some(lo), Some(hi))
            if lo.is_finite() && hi.is_finite() && lo <= p.receiver_vil_v && hi >= p.receiver_vih_v)
    }
}

#[derive(Debug, Clone)]
struct Pending {
    frame: SentFrame,
    any_transmission: bool,
    complete_transmission: bool,
}

impl Pending {
    fn observe(&mut self, transmits: bool) {
        self.any_transmission |= transmits;
        self.complete_transmission &= transmits;
    }

    fn finish(mut self) -> Option<SentFrame> {
        if !self.any_transmission { return None; }
        if !self.complete_transmission {
            // A physical interruption corrupts the protocol, rather than producing a forged
            // application error or an oracle decision. No recognized candidate is fabricated
            // for a line that remained completely silent.
            self.frame.intervals_us[0] = 0.0;
        }
        Some(self.frame)
    }
}

/// At most one pending frame. Reset restarts the clock; tri-state leaves it running.
#[derive(Debug, Clone)]
pub(crate) struct SentGenerator {
    p: SasSensorConfig,
    source: SasConfig,
    in_reset: bool,
    next_start: Option<u64>,
    counter: u32,
    first_after_reset: bool,
    pending: Option<Pending>,
    last_until: Option<u64>,
}

impl SentGenerator {
    pub(crate) fn new(p: SasSensorConfig, source: &SasConfig) -> Result<Self, String> {
        p.validate(source)?;
        Ok(Self { p, source: source.clone(), in_reset: true, next_start: None, counter: 0,
            first_after_reset: true, pending: None, last_until: None })
    }

    fn data(&self, angle_deg: f64) -> u32 {
        let s = &self.source.sent;
        let zero = (self.p.data_zero - s.data_min as i64) as f64 * self.p.turn_deg
            / self.source.wrap_steps as f64;
        let theta = (zero + self.p.mounting_direction as f64 * angle_deg
            + self.p.fault_angle_deg + self.p.static_error_deg).rem_euclid(self.p.turn_deg);
        (s.data_min as f64 + (theta * self.source.wrap_steps as f64 / self.p.turn_deg).round())
            .clamp(s.data_min as f64, s.data_max as f64) as u32
    }

    fn crc_transform(s: &SentConfig, mut crc: u32) -> u32 {
        let top = 1 << (s.nibble_bits - 1);
        let mask = (1 << s.nibble_bits) - 1;
        for _ in 0..s.nibble_bits {
            crc = if crc & top != 0 { (crc << 1) ^ s.crc_polynomial } else { crc << 1 };
            crc &= mask;
        }
        crc
    }

    fn make_frame(&self, started_us: u64, angle_deg: f64) -> SentFrame {
        let s = &self.source.sent;
        let mask = (1 << s.nibble_bits) - 1;
        let mut nibbles = vec![0; s.nibble_count];
        let put = |n: &mut Vec<u32>, indices: &[usize], mut value: u32| {
            for i in indices.iter().rev() { n[*i] = value & mask; value >>= s.nibble_bits; }
        };
        let data = if self.p.sensor_error { s.error_code } else { self.data(angle_deg) };
        put(&mut nibbles, &s.data_indices, data);
        put(&mut nibbles, &s.counter_indices, self.counter);
        nibbles[s.status_index] = if self.first_after_reset || self.p.sensor_error { s.error_mask } else { 0 };
        nibbles[s.inverse_index] = !nibbles[s.inverted_index] & mask;
        let mut crc = s.crc_seed;
        for i in &s.crc_indices { crc = Self::crc_transform(s, crc) ^ nibbles[*i]; }
        if s.crc_final_zero { crc = Self::crc_transform(s, crc); }
        nibbles[s.crc_index] = crc;
        SentFrame { started_us,
            completed_us: started_us.saturating_add((s.frame_ticks as f64 * self.p.tick_us).round() as u64),
            sync_us: s.sync_ticks as f64 * self.p.tick_us,
            intervals_us: nibbles.iter().map(|n| (s.nibble_base_ticks + n) as f64 * self.p.tick_us).collect() }
    }

    /// Electrical measurements apply on [from_us, until_us); angle interpolates between the
    /// actual mechanical positions at the start and end of the step. Calls cover successive physical
    /// steps. A gap is left silent; a long interval cannot manufacture unobserved history.
    pub(crate) fn advance(&mut self, from_us: u64, until_us: u64, m: SasMeasurement) -> Vec<SentFrame> {
        if until_us < from_us || until_us - from_us > self.p.max_advance_us
            || !m.angle_start_deg.is_finite() || !m.angle_deg.is_finite() {
            self.pending = None;
            self.next_start = None;
            self.in_reset = true;
            self.last_until = Some(until_us);
            return Vec::new();
        }
        if self.last_until.is_some_and(|t| t != from_us) {
            self.pending = None;
            self.next_start = None;
            self.in_reset = true;
        }
        self.last_until = Some(until_us);
        let vdd = m.vdd_v.filter(|v| v.is_finite()).unwrap_or(0.0);
        if vdd < self.p.por_off_v {
            self.in_reset = true;
            self.next_start = None;
            self.counter = 0;
            self.first_after_reset = true;
        } else if self.in_reset && vdd >= self.p.por_on_v {
            self.in_reset = false;
            self.next_start = Some(from_us.saturating_add(self.p.boot_us).saturating_add(self.p.phase_us));
        }
        let tristate = vdd > self.p.overvoltage_v || m.out_v.is_some_and(|v|
            !v.is_finite() || v > vdd + self.p.out_overvoltage_margin_v);
        let transmits = !self.in_reset && !tristate && m.transmits(&self.p);
        if let Some(pending) = &mut self.pending { pending.observe(transmits); }
        let mut output = Vec::new();
        loop {
            if self.pending.as_ref().is_some_and(|p| p.frame.completed_us <= until_us) {
                if let Some(frame) = self.pending.take().and_then(Pending::finish) { output.push(frame); }
            }
            if self.pending.is_some() { break; }
            let Some(start) = self.next_start else { break; };
            if start >= until_us { break; }
            let fraction = if until_us == from_us { 0.0 } else {
                (start.saturating_sub(from_us) as f64 / (until_us - from_us) as f64).clamp(0.0, 1.0)
            };
            let angle = m.angle_start_deg + (m.angle_deg - m.angle_start_deg) * fraction;
            let frame = self.make_frame(start, angle);
            self.next_start = Some(frame.completed_us);
            self.counter = (self.counter + 1) % self.source.sent.counter_modulus;
            self.first_after_reset = false;
            self.pending = Some(Pending { frame, any_transmission: transmits, complete_transmission: transmits });
        }
        output
    }
}

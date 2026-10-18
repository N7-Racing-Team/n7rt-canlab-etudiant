//! Bench multimeter: two probes on pins (COM and VΩ), read in both disclosures (contract §3.8: mission mode keeps
//! the readings of the instruments placed). V DC is the mean of the pin-to-pin voltage over the last 100 frames,
//! like the integration of a meter; Ω is the resistance of the switched-off circuit between the probes, read only
//! when every supply of the bench is off (an ohmmeter on a live circuit shows nothing meaningful). Hidden wire
//! defects are part of the circuit measured: an open wire reads OL between its ends.

use std::collections::VecDeque;

use n7rt_circuit::{Circuit, Kind, PinRef, Value};
use serde::{Deserialize, Serialize};

use super::Reading;

/// Frames averaged by the V DC function (100 ms).
pub const METER_WINDOW: usize = 100;
/// Above this resistance the display shows « OL ».
pub const OHM_OVERLOAD: f64 = 20e6;
/// The display without a reading.
pub const NO_READING: &str = "—";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeterFn {
    #[default]
    VDc,
    Ohm,
}

/// The meter as the interface shows it and as a recorded measure captures it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MeterValue {
    pub com: Option<PinRef>,
    pub vohm: Option<PinRef>,
    pub function: MeterFn,
    /// The display: « 2.734 V », « 68.0 Ω », « OL » or « — ».
    pub display: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Meter {
    pub com: Option<PinRef>,
    pub vohm: Option<PinRef>,
    pub function: MeterFn,
    window: VecDeque<f64>,
    display: Option<String>,
}

impl Meter {
    pub fn set(&mut self, com: Option<PinRef>, vohm: Option<PinRef>, function: MeterFn) {
        *self = Meter { com, vohm, function, ..Meter::default() };
    }

    pub fn probes(&self) -> Option<(PinRef, PinRef)> {
        Some((self.vohm.clone()?, self.com.clone()?))
    }

    pub fn value(&self) -> MeterValue {
        MeterValue {
            com: self.com.clone(),
            vohm: self.vohm.clone(),
            function: self.function,
            display: self.display.clone().unwrap_or_else(|| NO_READING.into()),
        }
    }

    /// One voltage sample: a frame of RUN (`advance`, averaged) or a STOP preview (shown alone).
    pub fn volts(&mut self, advance: bool, reading: Reading<f64>) {
        match reading {
            Reading::Valid { v } if v.is_finite() => {
                if !advance {
                    self.window.clear();
                }
                if self.window.len() == METER_WINDOW {
                    self.window.pop_front();
                }
                self.window.push_back(v);
                let mean = self.window.iter().sum::<f64>() / self.window.len() as f64;
                self.display = Some(format_volts(mean));
            }
            _ => {
                self.window.clear();
                self.display = None;
            }
        }
    }

    /// The average starts again from the next sample (same probes and function): after a verification, nothing of
    /// the trajectory it drove stays in the mean.
    pub fn restart(&mut self) {
        self.window.clear();
        self.display = None;
    }

    /// The resistance between the probes, `None` when the bench is live or the value cannot be read.
    pub fn ohms(&mut self, ohm: Option<f64>) {
        self.window.clear();
        self.display = ohm.map(format_ohms);
    }
}

/// Three decimals below 10 V, two below 100 V, one above (a four-digit display).
pub fn format_volts(v: f64) -> String {
    let a = v.abs();
    let text = if a < 9.9995 {
        format!("{v:.3}")
    } else if a < 99.995 {
        format!("{v:.2}")
    } else {
        format!("{v:.1}")
    };
    // no « -0.000 »
    let text = if text.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { text.trim_start_matches('-').to_owned() } else { text };
    format!("{text} V")
}

/// Ω, kΩ or MΩ on four digits; « OL » at and above [`OHM_OVERLOAD`] or for an open circuit.
pub fn format_ohms(r: f64) -> String {
    if !r.is_finite() || r >= OHM_OVERLOAD {
        return "OL".into();
    }
    let r = r.max(0.0);
    if r < 999.95 {
        format!("{r:.1} Ω")
    } else if r < 9_999.5 {
        format!("{:.3} kΩ", r / 1e3)
    } else if r < 999_950.0 {
        format!("{:.1} kΩ", r / 1e3)
    } else if r < 9_999_500.0 {
        format!("{:.3} MΩ", r / 1e6)
    } else {
        format!("{:.2} MΩ", r / 1e6)
    }
}

/// Some supply of the bench is on (a DC supply output, or a board on USB).
pub(crate) fn energized(circuit: &Circuit) -> bool {
    circuit.components().values().any(|c| {
        let key = match c.kind {
            Kind::DcSupply => "on",
            Kind::EcuStm32 => "usb",
            _ => return false,
        };
        circuit.state(c.id).and_then(|s| s.get(key)) != Some(&Value::Bool(false))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_follow_a_four_digit_meter() {
        assert_eq!(format_volts(0.0), "0.000 V");
        assert_eq!(format_volts(-1e-12), "0.000 V");
        assert_eq!(format_volts(2.7341), "2.734 V");
        assert_eq!(format_volts(-0.4906), "-0.491 V");
        assert_eq!(format_volts(12.0), "12.00 V");
        assert_eq!(format_volts(250.0), "250.0 V");
        assert_eq!(format_ohms(0.004), "0.0 Ω");
        assert_eq!(format_ohms(68.0), "68.0 Ω");
        assert_eq!(format_ohms(4700.0), "4.700 kΩ");
        assert_eq!(format_ohms(200e3), "200.0 kΩ");
        assert_eq!(format_ohms(1.5e6), "1.500 MΩ");
        assert_eq!(format_ohms(10e6), "10.00 MΩ");
        assert_eq!(format_ohms(20e6), "OL");
        assert_eq!(format_ohms(f64::INFINITY), "OL");
    }

    #[test]
    fn volts_average_the_last_hundred_frames() {
        let mut m = Meter::default();
        for k in 0..150 {
            m.volts(true, Reading::Valid { v: if k < 50 { 9.0 } else { 1.0 } });
        }
        assert_eq!(m.value().display, "1.000 V", "the first 50 samples left the window");
        m.volts(true, Reading::Floating);
        assert_eq!(m.value().display, NO_READING);
        m.volts(false, Reading::Valid { v: 2.0 });
        assert_eq!(m.value().display, "2.000 V");
    }
}

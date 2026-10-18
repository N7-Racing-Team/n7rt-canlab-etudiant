//! Public stand-in for the reception of one message by the background ECUs (VCU, inverter): the last frame received
//! with the right identifier and length, decoded as it is. No E2E check, no state machine, no timeout, no
//! qualification: the supervision of the mission (ch. 01 §3) belongs to the private oracle, compiled in only by the
//! instructor builds (build.rs, docs/distribution.md §1). A public build therefore shows background decoys that react
//! to the bus without supervising it.

use crate::codec;
use crate::pack::{Frame, PackData};

#[derive(Debug, Clone)]
pub(crate) struct Monitor {
    pub frame: usize,
    id: u32,
    dlc: usize,
    last: Option<Vec<u8>>,
    /// Never set by the stand-in.
    pub rx_timeout: bool,
}

impl Monitor {
    pub fn new(pack: &PackData, frame: usize) -> Monitor {
        let f = &pack.frames[frame];
        Monitor { frame, id: f.id, dlc: f.dlc as usize, last: None, rx_timeout: false }
    }

    pub fn on_frame(&mut self, _t_us: u64, id: u32, _flags: u8, data: &[u8]) {
        if id == self.id && data.len() >= self.dlc {
            self.last = Some(data[..self.dlc].to_vec());
        }
    }

    pub fn on_tick(&mut self, _frame: &Frame, _t_us: u64) {}

    /// Payload of the last frame received, if any.
    pub fn fresh_data(&self) -> Option<&[u8]> {
        self.last.as_deref()
    }

    /// Decoded value of a signal of the last frame (`None` = no frame yet, or SNA).
    pub fn value(&self, frame: &Frame, signal: usize) -> Option<f64> {
        codec::read_value(&frame.signals[signal], self.fresh_data()?)
    }
}

//! Public stand-in for the pedal input of the background VCU: the travel of the first sensor channel as it is decoded
//! from the bus, clamped to 0…100 %, never a cut. The mission consumer logic (qualification, plausibility,
//! persistence, cut, recovery, retained value) belongs to the private oracle, compiled in only by the instructor
//! builds (build.rs, docs/distribution.md §1).

use std::sync::Arc;

use crate::pack::PackData;
use crate::supervision::Monitor;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct AppsOut {
    pub latched: bool,
    pub retained: f64,
}

#[derive(Clone)]
pub(crate) struct AppsCore {
    pack: Arc<PackData>,
    /// Monitor of the frame carrying the first travel, and the signal index.
    travel: Option<(Monitor, usize)>,
}

impl AppsCore {
    pub fn new(pack: Arc<PackData>) -> AppsCore {
        let travel = pack
            .sources
            .get(&pack.consumer.apps.travel_sources[0])
            .map(|&(fi, si)| (Monitor::new(&pack, fi), si));
        AppsCore { pack, travel }
    }

    pub fn on_frame(&mut self, t_us: u64, id: u32, flags: u8, data: &[u8]) {
        if let Some((m, _)) = &mut self.travel {
            m.on_frame(t_us, id, flags, data);
        }
    }

    pub fn on_tick(&mut self, _t_us: u64) -> AppsOut {
        let travel = self.travel.as_ref().and_then(|(m, si)| m.value(&self.pack.frames[m.frame], *si));
        AppsOut { latched: false, retained: travel.unwrap_or(0.0).clamp(0.0, 100.0) }
    }
}

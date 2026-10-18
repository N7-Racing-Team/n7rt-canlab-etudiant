//! Replay of a hidden vector on the mission bus: the frames go to the board exactly as they were rendered by the
//! instructor edition, at their own bus time. The Physical Lab is not driven and its clock is not taken.

use std::sync::Arc;

use n7rt_vehicle::BusFrame;

use crate::sim::{BusSource, SourceFactory};
use crate::workflow::vector::HiddenVector;

pub struct ReplayFactory {
    frames: Arc<Vec<BusFrame>>,
}

impl ReplayFactory {
    pub fn new(vector: &HiddenVector) -> ReplayFactory {
        ReplayFactory { frames: Arc::new(vector.frames.clone()) }
    }
}

impl SourceFactory for ReplayFactory {
    fn start(&mut self) -> Box<dyn BusSource> {
        Box::new(ReplaySource { frames: self.frames.clone(), next: 0 })
    }
}

struct ReplaySource {
    frames: Arc<Vec<BusFrame>>,
    next: usize,
}

impl BusSource for ReplaySource {
    fn advance(&mut self, t_us: u64) -> Vec<BusFrame> {
        let from = self.next;
        while self.frames.get(self.next).is_some_and(|f| f.t_us <= t_us) {
            self.next += 1;
        }
        self.frames[from..self.next].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t_us: u64, id: u32) -> BusFrame {
        BusFrame { t_us, id, flags: 0, dlc: 1, data: vec![id as u8] }
    }

    #[test]
    fn each_session_replays_the_whole_vector_in_time_order() {
        let v = HiddenVector::new(100, vec![frame(0, 1), frame(10, 2), frame(10, 3), frame(55, 4)]).unwrap();
        let mut factory = ReplayFactory::new(&v);
        for _ in 0..2 {
            let mut source = factory.start();
            assert_eq!(source.advance(0), vec![frame(0, 1)]);
            assert_eq!(source.advance(9), vec![]);
            assert_eq!(source.advance(10), vec![frame(10, 2), frame(10, 3)]);
            assert_eq!(source.advance(10), vec![]);
            assert_eq!(source.advance(1_000), vec![frame(55, 4)]);
            assert_eq!(source.advance(2_000), vec![]);
        }
    }
}

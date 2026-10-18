//! Hidden nominal test: trajectory of the world quantity (cold start, full sweep with holds at both extremes, then
//! random segments drawn from the seed, back to rest). The comparison of the board's slots with the reference belongs
//! to the private oracle (`compare`).

use crate::pack::PackData;
use crate::prng;

#[derive(Debug, Clone)]
pub(crate) struct Scenario {
    pub duration_us: u64,
    /// (ms, position 0…1), strictly increasing times, linear in between.
    points: Vec<(u64, f64)>,
    quantum: f64,
    stepped: bool,
}

impl Scenario {
    pub fn new(p: &PackData, seed: u64) -> Scenario {
        let n = &p.nominal;
        let q = n.position_quantum;
        let mut index = 0u64;
        let mut draw = |lo: u64, hi: u64| -> u64 {
            let v = prng::uniform_int(seed, prng::STREAM_NOMINAL, index, lo as i64, hi as i64) as u64;
            index += 1;
            v
        };
        let mut pts: Vec<(u64, f64)> = vec![(0, 0.0)];
        let mut t = n.cold_start_ms;
        pts.push((t, 0.0));
        t += n.hold_ms;
        pts.push((t, 0.0));
        t += draw(n.ramp_ms[0], n.ramp_ms[1]);
        pts.push((t, 1.0));
        t += n.hold_ms;
        pts.push((t, 1.0));
        t += draw(n.ramp_ms[0], n.ramp_ms[1]);
        pts.push((t, 0.0));
        t += n.hold_ms;
        pts.push((t, 0.0));
        let end = n.cold_start_ms + n.nominal_ms;
        let last_ramp_end = end - n.hold_ms;
        let steps = (1.0 / q).round() as u64;
        let mut current = 0.0;
        loop {
            let room = last_ramp_end.saturating_sub(t);
            if room < 2 * n.segment_ms[0] {
                break;
            }
            let d = draw(n.segment_ms[0], n.segment_ms[1]).min(room - n.segment_ms[0]);
            // one segment in three is a hold
            let target = if draw(0, 2) == 0 { current } else { draw(0, steps) as f64 * q };
            t += d;
            pts.push((t, target));
            current = target;
        }
        if last_ramp_end > t {
            pts.push((last_ramp_end, 0.0));
        }
        if end > pts.last().map_or(0, |x| x.0) {
            pts.push((end, 0.0));
        }
        Scenario { duration_us: end * 1000, points: pts, quantum: q, stepped: p.mission == "bps" }
    }

    pub fn position_at(&self, t_us: u64) -> f64 {
        let t = t_us as f64 / 1000.0;
        let pts = &self.points;
        let mut x = pts.last().map_or(0.0, |p| p.1);
        for w in pts.windows(2) {
            let (t0, p0) = (w[0].0 as f64, w[0].1);
            let (t1, p1) = (w[1].0 as f64, w[1].1);
            if t < t0 {
                x = p0;
                break;
            }
            if t <= t1 {
                x = if self.stepped { if t < t1 { p0 } else { p1 } }
                    else if t1 > t0 { p0 + (p1 - p0) * (t - t0) / (t1 - t0) } else { p1 };
                break;
            }
        }
        let k = (x.clamp(0.0, 1.0) / self.quantum + 0.5 + 1e-9).floor();
        (k * self.quantum).clamp(0.0, 1.0)
    }
}

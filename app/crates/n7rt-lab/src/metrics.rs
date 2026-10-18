//! Small fixed-memory metrics: latency histogram and sliding-window rate.

use std::time::{Duration, Instant};

/// Linear histogram in microseconds; values beyond the last bin count as overflow and are
/// reported at `max`.
#[derive(Debug, Clone)]
pub struct Histogram {
    bin_us: u32,
    bins: Vec<u64>,
    overflow: u64,
    count: u64,
    max_us: u32,
    sum_us: u64,
}

impl Histogram {
    pub fn new(bin_us: u32, range_us: u32) -> Self {
        Self { bin_us, bins: vec![0; (range_us / bin_us) as usize], overflow: 0, count: 0, max_us: 0, sum_us: 0 }
    }

    pub fn record(&mut self, us: u32) {
        let i = (us / self.bin_us) as usize;
        match self.bins.get_mut(i) {
            Some(b) => *b += 1,
            None => self.overflow += 1,
        }
        self.count += 1;
        self.max_us = self.max_us.max(us);
        self.sum_us += us as u64;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn max(&self) -> u32 {
        self.max_us
    }

    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum_us as f64 / self.count as f64
        }
    }

    /// Upper edge of the bin holding quantile `q` (conservative by at most one bin).
    pub fn quantile(&self, q: f64) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let rank = ((q * self.count as f64).ceil() as u64).clamp(1, self.count);
        let mut seen = 0;
        for (i, b) in self.bins.iter().enumerate() {
            seen += b;
            if seen >= rank {
                return ((i as u32 + 1) * self.bin_us).min(self.max_us.max(self.bin_us));
            }
        }
        self.max_us
    }

    pub fn reset(&mut self) {
        self.bins.iter_mut().for_each(|b| *b = 0);
        self.overflow = 0;
        self.count = 0;
        self.max_us = 0;
        self.sum_us = 0;
    }
}

/// Events per second over the last second, in 100 ms buckets.
#[derive(Debug, Clone)]
pub struct RateMeter {
    origin: Instant,
    buckets: [u64; 10],
    head: u64,
}

impl RateMeter {
    pub fn new() -> Self {
        Self { origin: Instant::now(), buckets: [0; 10], head: 0 }
    }

    fn advance(&mut self, now: Instant) {
        let idx = (now.saturating_duration_since(self.origin).as_millis() / 100) as u64;
        if idx > self.head {
            for k in (self.head + 1)..=idx.min(self.head + 10) {
                self.buckets[(k % 10) as usize] = 0;
            }
            self.head = idx;
        }
    }

    pub fn add(&mut self, now: Instant, n: u64) {
        self.advance(now);
        self.buckets[(self.head % 10) as usize] += n;
    }

    /// Rate over the 9 complete buckets plus the current one, scaled to one second.
    pub fn per_second(&mut self, now: Instant) -> f64 {
        self.advance(now);
        let elapsed = now.saturating_duration_since(self.origin);
        let window = elapsed.min(Duration::from_secs(1)).as_secs_f64().max(0.1);
        self.buckets.iter().sum::<u64>() as f64 / window
    }
}

impl Default for RateMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles() {
        let mut h = Histogram::new(10, 1000);
        for us in 1..=100 {
            h.record(us * 5);
        }
        assert_eq!(h.count(), 100);
        // Upper edge of the 10 µs bin holding 250 µs.
        assert_eq!(h.quantile(0.5), 260);
        assert_eq!(h.quantile(0.99), 500);
        assert_eq!(h.max(), 500);
        h.record(5000);
        assert_eq!(h.quantile(1.0), 5000);
    }
}

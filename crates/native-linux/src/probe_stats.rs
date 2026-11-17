//! Fixed-size latency histogram, including for multi-hour development runs.
pub struct Histogram {
    bins: [u64; 4001],
    count: u64,
    maximum: f64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            bins: [0; 4001],
            count: 0,
            maximum: 0.0,
        }
    }
}

impl Histogram {
    pub fn record(&mut self, milliseconds: f64) {
        assert!(milliseconds.is_finite() && milliseconds >= 0.0);
        let index = (milliseconds * 4.0).ceil().min(4000.0) as usize;
        self.bins[index] += 1;
        self.count += 1;
        self.maximum = self.maximum.max(milliseconds);
    }
    pub fn p95(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let rank = (self.count as f64 * 0.95).ceil() as u64;
        let mut total = 0;
        for (index, count) in self.bins.iter().enumerate() {
            total += count;
            if total >= rank {
                return Some(if index == 4000 {
                    self.maximum
                } else {
                    index as f64 / 4.0
                });
            }
        }
        unreachable!()
    }
    pub fn count(&self) -> u64 {
        self.count
    }
    pub fn maximum(&self) -> Option<f64> {
        (self.count > 0).then_some(self.maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_running_statistics_stay_bounded_and_preserve_spikes() {
        let mut histogram = Histogram::default();
        assert!(histogram.p95().is_none());
        for _ in 0..2_000_000 {
            histogram.record(16.13);
        }
        histogram.record(2500.0);
        assert_eq!(histogram.p95(), Some(16.25));
        assert_eq!(histogram.maximum(), Some(2500.0));
        assert_eq!(histogram.count(), 2_000_001);
        assert!(std::mem::size_of_val(&histogram) < 33_000);
    }
}

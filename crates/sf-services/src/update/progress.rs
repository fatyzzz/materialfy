//! Download progress math: smoothed speed (exponential moving average over time) and time left.

/// Smoothing time constant, seconds: the speed follows changes within a few seconds without jitter.
const TAU_S: f64 = 2.0;

#[derive(Clone, Debug, Default)]
pub struct RateMeter {
    last_t: Option<f64>,
    last_bytes: u64,
    rate: Option<f64>,
}

impl RateMeter {
    /// New sample: `t` — seconds since any fixed point (monotonic), `bytes` — total received so far.
    /// Returns the smoothed rate in bytes/s (None until two samples are at least 50 ms apart).
    pub fn sample(&mut self, t: f64, bytes: u64) -> Option<f64> {
        let Some(last_t) = self.last_t else {
            self.last_t = Some(t);
            self.last_bytes = bytes;
            return self.rate;
        };
        let dt = t - last_t;
        if dt < 0.05 {
            return self.rate;
        }
        let instant = bytes.saturating_sub(self.last_bytes) as f64 / dt;
        self.rate = Some(match self.rate {
            None => instant,
            Some(prev) => ewma(prev, instant, dt, TAU_S),
        });
        self.last_t = Some(t);
        self.last_bytes = bytes;
        self.rate
    }

    pub fn rate(&self) -> Option<f64> {
        self.rate
    }
}

/// Exponential moving average with a time constant: the weight of the new value grows with the gap `dt`.
pub fn ewma(prev: f64, value: f64, dt: f64, tau: f64) -> f64 {
    let alpha = 1.0 - (-dt.max(0.0) / tau).exp();
    prev + alpha * (value - prev)
}

/// Seconds left (rounded up); None — unknown (no rate yet or the download stalled).
pub fn eta_secs(done: u64, total: u64, rate: Option<f64>) -> Option<u64> {
    let rate = rate.filter(|r| r.is_finite() && *r >= 1.0)?;
    if total == 0 {
        return None;
    }
    let left = total.saturating_sub(done) as f64;
    Some((left / rate).ceil() as u64)
}

/// 0..1; 0 when the size is unknown.
pub fn fraction(done: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_rate() {
        let mut meter = RateMeter::default();
        assert_eq!(meter.sample(0.0, 0), None);
        // too close to the previous sample
        assert_eq!(meter.sample(0.01, 10_000), None);
        let first = meter.sample(1.0, 1_000_000).unwrap();
        assert!((first - 1_000_000.0).abs() < 1.0);
        for i in 2..20 {
            meter.sample(i as f64, i * 1_000_000);
        }
        assert!((meter.rate().unwrap() - 1_000_000.0).abs() < 1.0);
    }

    #[test]
    fn smooths_spikes_and_follows_changes() {
        let mut meter = RateMeter::default();
        meter.sample(0.0, 0);
        meter.sample(1.0, 1_000_000);
        // one 0.1 s burst at 10 MB/s barely moves the rate
        let after_spike = meter.sample(1.1, 2_000_000).unwrap();
        assert!(after_spike < 6_000_000.0 && after_spike > 1_000_000.0, "{after_spike}");
        // a stalled connection drives it towards zero within seconds
        let mut t = 1.1;
        while t < 12.0 {
            t += 0.5;
            meter.sample(t, 2_000_000);
        }
        assert!(meter.rate().unwrap() < 50_000.0);
    }

    #[test]
    fn ewma_weights() {
        assert_eq!(ewma(10.0, 20.0, 0.0, 2.0), 10.0);
        assert!((ewma(10.0, 20.0, 1000.0, 2.0) - 20.0).abs() < 1e-9);
        let half = ewma(0.0, 1.0, 2.0 * std::f64::consts::LN_2, 2.0);
        assert!((half - 0.5).abs() < 1e-9);
    }

    #[test]
    fn eta_and_fraction() {
        assert_eq!(eta_secs(0, 28_000_000, Some(3_100_000.0)), Some(10));
        assert_eq!(eta_secs(12_400_000, 28_000_000, Some(3_100_000.0)), Some(6));
        assert_eq!(eta_secs(28_000_000, 28_000_000, Some(3_100_000.0)), Some(0));
        assert_eq!(eta_secs(1, 10, None), None);
        assert_eq!(eta_secs(1, 10, Some(0.2)), None);
        assert_eq!(eta_secs(1, 10, Some(f64::NAN)), None);
        assert_eq!(eta_secs(1, 0, Some(100.0)), None);
        assert_eq!(fraction(0, 0), 0.0);
        assert_eq!(fraction(5, 10), 0.5);
        assert_eq!(fraction(15, 10), 1.0);
    }
}

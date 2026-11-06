// src/temporal.rs — Temporal Signal Calculus & Rigorous Semantics for Aven
// Milestone Phase 6: Sample retention windows, staleness decay models, and cross-signal alignment.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub struct TemporalSample {
    pub timestamp_ms: u64,
    pub value: f64,
    pub ttl_ms: Option<u64>,
}

impl TemporalSample {
    pub fn new(timestamp_ms: u64, value: f64, ttl_ms: Option<u64>) -> Self {
        Self {
            timestamp_ms,
            value,
            ttl_ms,
        }
    }

    /// Computes age of sample relative to current timestamp.
    pub fn age_ms(&self, current_time_ms: u64) -> u64 {
        current_time_ms.saturating_sub(self.timestamp_ms)
    }

    /// Checks if sample has exceeded its nominal TTL.
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        match self.ttl_ms {
            Some(ttl) => self.age_ms(current_time_ms) > ttl,
            None => false,
        }
    }
}

/// Staleness decay models that calculate confidence/freshness score in [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum StalenessDecay {
    Step,
    #[default]
    Linear,
    Exponential {
        half_life_ms: f64,
    },
}

impl StalenessDecay {
    /// Computes confidence in [0.0, 1.0].
    pub fn confidence(&self, age_ms: u64, ttl_ms: u64) -> f64 {
        if ttl_ms == 0 {
            return if age_ms == 0 { 1.0 } else { 0.0 };
        }
        let age = age_ms as f64;
        let ttl = ttl_ms as f64;

        match self {
            StalenessDecay::Step => {
                if age <= ttl {
                    1.0
                } else {
                    0.0
                }
            }
            StalenessDecay::Linear => {
                if age >= ttl {
                    0.0
                } else {
                    (1.0 - (age / ttl)).clamp(0.0, 1.0)
                }
            }
            StalenessDecay::Exponential { half_life_ms } => {
                let hl = if *half_life_ms <= 0.0 {
                    ttl / 2.0
                } else {
                    *half_life_ms
                };
                let lambda = 2.0_f64.ln() / hl.max(1e-6);
                (-lambda * age).exp().clamp(0.0, 1.0)
            }
        }
    }
}

/// Bounded sliding FIFO history buffer for a temporal signal stream.
#[derive(Debug, Clone)]
pub struct RetentionWindow {
    pub max_duration_ms: Option<u64>,
    pub max_samples: usize,
    samples: Vec<TemporalSample>,
}

impl RetentionWindow {
    pub fn new(max_duration_ms: Option<u64>, max_samples: usize) -> Self {
        Self {
            max_duration_ms,
            max_samples: max_samples.max(1),
            samples: Vec::new(),
        }
    }

    /// Pushes a new sample into the retention window and evicts stale/overflow items.
    pub fn push(&mut self, sample: TemporalSample) {
        // Enforce chronological insertion
        if let Some(last) = self.samples.last() {
            if sample.timestamp_ms < last.timestamp_ms {
                // Out-of-order sample: insert sorted
                let idx = self
                    .samples
                    .binary_search_by_key(&sample.timestamp_ms, |s| s.timestamp_ms)
                    .unwrap_or_else(|i| i);
                self.samples.insert(idx, sample);
            } else {
                self.samples.push(sample);
            }
        } else {
            self.samples.push(sample);
        }

        self.evict();
    }

    /// Evicts samples exceeding count capacity or time retention limit.
    fn evict(&mut self) {
        if let Some(dur) = self.max_duration_ms {
            if let Some(latest) = self.samples.last() {
                let cutoff = latest.timestamp_ms.saturating_sub(dur);
                let keep_idx = self
                    .samples
                    .iter()
                    .position(|s| s.timestamp_ms >= cutoff)
                    .unwrap_or(self.samples.len());
                if keep_idx > 0 {
                    self.samples.drain(0..keep_idx);
                }
            }
        }

        while self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    pub fn latest(&self) -> Option<&TemporalSample> {
        self.samples.last()
    }

    pub fn oldest(&self) -> Option<&TemporalSample> {
        self.samples.first()
    }

    pub fn samples(&self) -> &[TemporalSample] {
        &self.samples
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Computes the discrete rate of change (slope: dv / dt in units/second).
    pub fn rate_of_change(&self) -> Option<f64> {
        if self.samples.len() < 2 {
            return None;
        }
        let oldest = self.samples.first()?;
        let newest = self.samples.last()?;
        let dt_ms = newest.timestamp_ms.saturating_sub(oldest.timestamp_ms);
        if dt_ms == 0 {
            return Some(0.0);
        }
        let dt_sec = dt_ms as f64 / 1000.0;
        let dv = newest.value - oldest.value;
        Some(dv / dt_sec)
    }

    /// Computes arithmetic moving average of values in window.
    pub fn moving_average(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: f64 = self.samples.iter().map(|s| s.value).sum();
        Some(sum / self.samples.len() as f64)
    }

    /// Computes exponential moving average (EMA) with smoothing factor alpha in (0, 1].
    pub fn exponential_smoothing(&self, alpha: f64) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let a = alpha.clamp(0.01, 1.0);
        let mut ema = self.samples[0].value;
        for s in &self.samples[1..] {
            ema = a * s.value + (1.0 - a) * ema;
        }
        Some(ema)
    }

    /// Computes sample variance across the window.
    pub fn variance(&self) -> Option<f64> {
        if self.samples.len() < 2 {
            return Some(0.0);
        }
        let avg = self.moving_average()?;
        let sq_diff_sum: f64 = self.samples.iter().map(|s| (s.value - avg).powi(2)).sum();
        Some(sq_diff_sum / (self.samples.len() - 1) as f64)
    }

    /// Computes standard deviation across the window.
    pub fn std_dev(&self) -> Option<f64> {
        self.variance().map(|v| v.sqrt())
    }
}

/// Aligned pair of cross-stream temporal samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignedSignalPair {
    pub timestamp_ms: u64,
    pub value_a: f64,
    pub value_b: f64,
    pub skew_ms: i64,
    pub within_tolerance: bool,
}

/// Temporal cross-signal alignment algorithm.
/// Aligns samples from stream_a with the temporally closest sample in stream_b.
pub fn align_signals(
    stream_a: &[TemporalSample],
    stream_b: &[TemporalSample],
    tolerance_ms: u64,
) -> Vec<AlignedSignalPair> {
    let mut aligned = Vec::new();
    if stream_a.is_empty() || stream_b.is_empty() {
        return aligned;
    }

    for sa in stream_a {
        // Find closest sample in stream_b
        let mut best_b = &stream_b[0];
        let mut min_diff = (sa.timestamp_ms as i64 - best_b.timestamp_ms as i64).abs();

        for sb in stream_b.iter().skip(1) {
            let diff = (sa.timestamp_ms as i64 - sb.timestamp_ms as i64).abs();
            if diff < min_diff {
                min_diff = diff;
                best_b = sb;
            } else if diff > min_diff {
                // b is sorted; once diff increases we can break
                break;
            }
        }

        let skew_ms = sa.timestamp_ms as i64 - best_b.timestamp_ms as i64;
        let within_tolerance = (skew_ms.unsigned_abs()) <= tolerance_ms;

        aligned.push(AlignedSignalPair {
            timestamp_ms: sa.timestamp_ms,
            value_a: sa.value,
            value_b: best_b.value,
            skew_ms,
            within_tolerance,
        });
    }

    aligned
}

/// Supervisor tracking multiple named temporal streams with actionability gates.
#[derive(Debug, Clone)]
pub struct TemporalSupervisor {
    pub confidence_threshold: f64,
    decay_model: StalenessDecay,
    signals: std::collections::HashMap<String, RetentionWindow>,
}

impl Default for TemporalSupervisor {
    fn default() -> Self {
        Self::new(0.5, StalenessDecay::Linear)
    }
}

impl TemporalSupervisor {
    pub fn new(confidence_threshold: f64, decay_model: StalenessDecay) -> Self {
        Self {
            confidence_threshold: confidence_threshold.clamp(0.0, 1.0),
            decay_model,
            signals: std::collections::HashMap::new(),
        }
    }

    /// Registers or updates a named signal with a specific retention configuration.
    pub fn register_signal(
        &mut self,
        name: &str,
        max_duration_ms: Option<u64>,
        max_samples: usize,
    ) {
        self.signals
            .entry(name.to_string())
            .or_insert_with(|| RetentionWindow::new(max_duration_ms, max_samples));
    }

    /// Ingests a new temporal sample for a named signal.
    pub fn ingest(&mut self, name: &str, sample: TemporalSample) {
        let window = self
            .signals
            .entry(name.to_string())
            .or_insert_with(|| RetentionWindow::new(Some(60000), 100));
        window.push(sample);
    }

    /// Evaluates current staleness confidence score for a signal in [0.0, 1.0].
    pub fn confidence(&self, name: &str, current_time_ms: u64) -> f64 {
        let Some(window) = self.signals.get(name) else {
            return 0.0;
        };
        let Some(latest) = window.latest() else {
            return 0.0;
        };
        let ttl = latest.ttl_ms.unwrap_or(1000);
        let age = latest.age_ms(current_time_ms);
        self.decay_model.confidence(age, ttl)
    }

    /// Determines whether a signal is actionable (confidence >= threshold).
    pub fn is_actionable(&self, name: &str, current_time_ms: u64) -> bool {
        self.confidence(name, current_time_ms) >= self.confidence_threshold
    }

    /// Accesses the underlying retention window for a signal.
    pub fn window(&self, name: &str) -> Option<&RetentionWindow> {
        self.signals.get(name)
    }
}

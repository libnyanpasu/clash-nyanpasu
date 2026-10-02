//! Monotonic startup measurements, owned by the bootstrap rather than a global.
use parking_lot::Mutex;
use std::{collections::HashSet, time::Instant};

#[derive(Default)]
struct Records {
    pending: Option<Vec<Measurement>>,
    milestones: HashSet<&'static str>,
}

pub struct StartupTimings {
    started: Instant,
    records: Mutex<Records>,
}

impl StartupTimings {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            records: Mutex::new(Records {
                pending: Some(Vec::new()),
                ..Records::default()
            }),
        }
    }

    pub fn stage(&self, name: &'static str) -> StartupStage<'_> {
        StartupStage {
            name,
            started: Instant::now(),
            timings: Some(self),
        }
    }

    /// Replay measurements made before the application's logger existed.
    pub fn enable_logging(&self) {
        let pending = self.records.lock().pending.take().unwrap_or_default();
        for measurement in pending {
            measurement.emit();
        }
    }

    fn record(&self, measurement: Measurement) {
        let mut records = self.records.lock();
        if let Some(pending) = &mut records.pending {
            pending.push(measurement);
        } else {
            drop(records);
            measurement.emit();
        }
    }

    /// Later reloads, window reopenings and tray rebuilds are not startup.
    pub fn milestone(&self, name: &'static str) {
        if self.records.lock().milestones.insert(name) {
            tracing::info!(
                target: "clash_nyanpasu::startup",
                pid = std::process::id(),
                milestone = name,
                elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0,
                "startup milestone"
            );
        }
    }
}

struct Measurement {
    stage: &'static str,
    elapsed_ms: f64,
    start_ms: Option<f64>,
    end_ms: Option<f64>,
}

impl Measurement {
    fn emit(&self) {
        tracing::info!(
            target: "clash_nyanpasu::startup",
            pid = std::process::id(),
            stage = self.stage,
            elapsed_ms = self.elapsed_ms,
            start_ms = self.start_ms,
            end_ms = self.end_ms,
            "startup stage finished"
        );
    }
}

/// A scope duration includes awaited work and is also recorded on early return.
/// Finishing the scope does not imply that the operation succeeded.
pub struct StartupStage<'a> {
    name: &'static str,
    started: Instant,
    timings: Option<&'a StartupTimings>,
}

impl StartupStage<'static> {
    /// Constructor details have their own duration, without a process origin.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
            timings: None,
        }
    }
}

impl Drop for StartupStage<'_> {
    fn drop(&mut self) {
        let ended = Instant::now();
        let measurement = Measurement {
            stage: self.name,
            elapsed_ms: ended.duration_since(self.started).as_secs_f64() * 1000.0,
            start_ms: self
                .timings
                .map(|timings| self.started.duration_since(timings.started).as_secs_f64() * 1000.0),
            end_ms: self
                .timings
                .map(|timings| ended.duration_since(timings.started).as_secs_f64() * 1000.0),
        };
        if let Some(timings) = self.timings {
            timings.record(measurement);
        } else {
            measurement.emit();
        }
    }
}

#[cfg(test)]
mod tests;

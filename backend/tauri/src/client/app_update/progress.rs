//! Download speed sampling for one application update attempt.

use std::{collections::VecDeque, time::Duration};

use tokio::time::Instant;

/// Interval between progress samples sent to the update owner.
pub(super) const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// Speed averages the bytes received over this window, so a stall of this
/// length reads as zero.
const SPEED_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ProgressSample {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Effective package bytes per second for the current attempt.
    pub speed: f64,
}

/// Cumulative progress and sliding-window speed of one download attempt.
///
/// Speed comes from the sample clock, never from when a message is handled,
/// so bursts or a delayed consumer cannot shrink the measured interval.
pub(super) struct ProgressSampler {
    downloaded: u64,
    total: Option<u64>,
    /// Samples within the speed window, oldest first; never empty.
    window: VecDeque<(Instant, u64)>,
}

impl ProgressSampler {
    pub fn new(started_at: Instant) -> Self {
        Self {
            downloaded: 0,
            total: None,
            window: VecDeque::from([(started_at, 0)]),
        }
    }

    /// Records the attempt's cumulative bytes. They never decrease within an
    /// attempt, so a report that arrives out of order is ignored.
    pub fn record(&mut self, downloaded: u64, total: Option<u64>) {
        self.downloaded = self.downloaded.max(downloaded);
        self.total = total;
    }

    pub fn sample(&mut self, now: Instant) -> ProgressSample {
        while self
            .window
            .get(1)
            .is_some_and(|&(at, _)| now.saturating_duration_since(at) >= SPEED_WINDOW)
        {
            self.window.pop_front();
        }
        let (since, base) = self.window[0];
        let elapsed = now.saturating_duration_since(since);
        // A window shorter than one interval only happens right after an
        // attempt starts between ticks; its rate would turn a burst into a spike.
        let speed = if elapsed < SAMPLE_INTERVAL {
            0.0
        } else {
            (self.downloaded - base) as f64 / elapsed.as_secs_f64()
        };
        self.window.push_back((now, self.downloaded));
        ProgressSample {
            downloaded: self.downloaded,
            total: self.total,
            speed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;
    const MIB: f64 = 1024.0 * 1024.0;

    fn tick(start: Instant, n: u32) -> Instant {
        start + SAMPLE_INTERVAL * n
    }

    #[test]
    fn steady_progress_reads_its_rate() {
        let start = Instant::now();
        let mut sampler = ProgressSampler::new(start);
        for n in 1..=8 {
            sampler.record(256 * KIB * u64::from(n), Some(4 * 1024 * KIB));
            let sample = sampler.sample(tick(start, n));
            assert_eq!(sample.speed, MIB);
            assert_eq!(sample.downloaded, 256 * KIB * u64::from(n));
            assert_eq!(sample.total, Some(4 * 1024 * KIB));
        }
    }

    #[test]
    fn a_burst_reads_its_window_average_and_a_stall_reads_zero() {
        let start = Instant::now();
        let mut sampler = ProgressSampler::new(start);
        sampler.record(1024 * KIB, None);
        assert_eq!(sampler.sample(tick(start, 1)).speed, 4.0 * MIB);
        assert_eq!(sampler.sample(tick(start, 2)).speed, 2.0 * MIB);
        assert_eq!(sampler.sample(tick(start, 3)).speed, 4.0 * MIB / 3.0);
        assert_eq!(sampler.sample(tick(start, 4)).speed, MIB);
        // One second after the last byte the whole window is idle.
        assert_eq!(sampler.sample(tick(start, 5)).speed, 0.0);

        sampler.record(1024 * KIB + 256 * KIB, None);
        assert_eq!(sampler.sample(tick(start, 6)).speed, 256.0 * 1024.0);
    }

    #[test]
    fn a_window_shorter_than_one_interval_reads_zero() {
        let start = Instant::now();
        let mut sampler = ProgressSampler::new(start);
        sampler.record(64 * KIB, None);
        let early = sampler.sample(start + Duration::from_millis(10));
        assert_eq!(early.speed, 0.0);
        assert_eq!(early.downloaded, 64 * KIB);
        assert_eq!(sampler.sample(start).speed, 0.0);
    }

    #[test]
    fn out_of_order_reports_do_not_move_progress_backwards() {
        let start = Instant::now();
        let mut sampler = ProgressSampler::new(start);
        sampler.record(512 * KIB, Some(1024 * KIB));
        sampler.record(256 * KIB, Some(1024 * KIB));
        let sample = sampler.sample(tick(start, 1));
        assert_eq!(sample.downloaded, 512 * KIB);
        assert_eq!(sample.speed, 2.0 * MIB);
    }
}

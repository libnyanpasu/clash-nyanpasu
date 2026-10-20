//! Resource profiles (§10.1): budgets and limits for each platform tier (desktop, mobile,
//! x86 device, and the untested tier-2 low-end device). The host chooses one and injects
//! it, and every value can be overridden.
//!
//! A preset is a starting point: the design lists its values as initial ones, and the
//! benchmarks of §12 inform recommendations; no preset or floor is calibrated by default.
//! Measurements and verification limits live in the crate's BENCHMARKS.md. A field left to
//! those benchmarks says so in its doc.
//!
//! The table of §10.1 writes the desktop and x86 budgets in MB and the mobile budget in
//! MiB, and the presets keep those units literally: 1 MB is 1,000,000 bytes and 1 MiB is
//! 1,048,576. The mobile total is a hard ceiling set by the iOS extension, so it is the
//! one counted in binary units.

use std::time::Duration;

use snafu::{Snafu, ensure};

use crate::ErrorKind;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const MB: u64 = 1_000_000;

// The values shared by every preset (§4.2, §4.3, §9.1).
const EVENT_BUFFER_ROWS: usize = 4_096;
const EVENT_BUFFER_BYTES: u64 = 64 * KIB;
const EVENT_BUFFER_AGE: Duration = Duration::from_secs(60 * 60);
const CONSOLIDATE_CHUNKS: usize = 64;
const CONSOLIDATE_BYTES: u64 = 16 * MIB;
const MAX_SORTED_RUNS: usize = 8;
const CLEANUP_CHUNKS: usize = 64;

/// The budgets and limits of one store. Build one with a preset and override fields, then
/// check it with [`validate`](Self::validate).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The head: the in-memory view of the journal records that are not in a chunk yet
    /// (§4.2).
    pub head: u64,
    /// The catalog: label dictionaries and series combinations (§5.2).
    pub catalog: u64,
    /// The page cache the backend is told to use. The table's "redb cache" line is
    /// `backend_cache + backend_overhead`: the overhead is carved out of it.
    pub backend_cache: u64,
    /// The memory the backend uses outside its cache: allocator state, transaction page
    /// tracking and the pages of non-durable commits (§7.4). The design leaves the value
    /// to the benchmarks; the presets start from a guess.
    pub backend_overhead: u64,
    /// All concurrent queries together, including the old versions their snapshots keep
    /// alive.
    pub query: u64,
    /// Staging, encode buffers and compression contexts.
    pub staging: u64,

    /// The cap on journal bytes in storage that are not in a chunk yet. It bounds the
    /// replay on restart, not memory (§4.2). It follows the table.
    pub journal_bytes: u64,

    /// The largest single batch. A consumer flushes early near it, because a batch is one
    /// transaction and cannot be split (§4.2). The benchmarks set the value.
    pub batch_bytes: u64,

    /// The longest time between durable commits; `None` makes every commit durable. In
    /// between, commits are only crash-consistent (§3.2), so this is also the longest
    /// time of data a crash can lose.
    pub durable_interval: Option<Duration>,
    /// Written since the last durable commit before the next commit is made durable
    /// without waiting for `durable_interval` (§7.4). The interval alone does not bound
    /// the pages a backend accumulates. Moot while every commit is durable. The
    /// benchmarks set the value.
    pub durable_after_bytes: u64,

    /// An event stream's buffer is turned into a chunk at this many rows ...
    pub event_buffer_rows: usize,
    /// ... or this many bytes ...
    pub event_buffer_bytes: u64,
    /// ... or when its oldest row is this old (§4.2, the time T).
    pub event_buffer_age: Duration,

    /// How long a series bucket stays quiet after its end before it is made a chunk (the
    /// time Q of §4.2). `None` is one bucket width, which differs per tier, so it cannot
    /// be written as a duration here.
    pub series_quiet_time: Option<Duration>,

    /// One consolidation round takes at most this many input chunks (F, §4.3) ...
    pub consolidate_chunks: usize,
    /// ... and this many input bytes (I).
    pub consolidate_bytes: u64,
    /// More sorted runs than this in an extent make maintenance merge the smallest (R,
    /// §4.3).
    pub max_sorted_runs: usize,
    /// One cleanup round deletes at most this many chunks (D, §9.1).
    pub cleanup_chunks: usize,
}

impl Profile {
    /// Windows, macOS and Linux on x86_64 and aarch64: 192 MB.
    pub fn desktop() -> Self {
        Self {
            head: 32 * MB,
            catalog: 32 * MB,
            backend_cache: 56 * MB,
            backend_overhead: 8 * MB,
            query: 56 * MB,
            staging: 8 * MB,
            journal_bytes: 64 * MB,
            batch_bytes: 4 * MIB,
            durable_interval: None,
            durable_after_bytes: 64 * MIB,
            event_buffer_rows: EVENT_BUFFER_ROWS,
            event_buffer_bytes: EVENT_BUFFER_BYTES,
            event_buffer_age: EVENT_BUFFER_AGE,
            series_quiet_time: None,
            consolidate_chunks: CONSOLIDATE_CHUNKS,
            consolidate_bytes: CONSOLIDATE_BYTES,
            max_sorted_runs: MAX_SORTED_RUNS,
            cleanup_chunks: CLEANUP_CHUNKS,
        }
    }

    /// Android and the iOS network extension: 8 MiB, because the extension shares 50 MiB
    /// with the proxy core. Commits are crash-consistent, and a durable one comes at least
    /// every five minutes (and before the extension suspends, which the host triggers).
    pub fn mobile() -> Self {
        Self {
            head: 3 * MIB / 2,
            catalog: 3 * MIB / 2,
            backend_cache: MIB,
            backend_overhead: 512 * KIB,
            query: 5 * MIB / 2,
            staging: MIB,
            journal_bytes: 4 * MIB,
            batch_bytes: 256 * KIB,
            durable_interval: Some(Duration::from_secs(5 * 60)),
            durable_after_bytes: 2 * MIB,
            event_buffer_rows: EVENT_BUFFER_ROWS,
            event_buffer_bytes: EVENT_BUFFER_BYTES,
            event_buffer_age: EVENT_BUFFER_AGE,
            series_quiet_time: None,
            consolidate_chunks: CONSOLIDATE_CHUNKS,
            consolidate_bytes: CONSOLIDATE_BYTES,
            max_sorted_runs: MAX_SORTED_RUNS,
            cleanup_chunks: CLEANUP_CHUNKS,
        }
    }

    /// Soft routers and small x86 hosts: 96 MB.
    pub fn x86_device() -> Self {
        Self {
            head: 16 * MB,
            catalog: 16 * MB,
            backend_cache: 28 * MB,
            backend_overhead: 4 * MB,
            query: 28 * MB,
            staging: 4 * MB,
            journal_bytes: 32 * MB,
            batch_bytes: 2 * MIB,
            durable_interval: None,
            durable_after_bytes: 32 * MIB,
            event_buffer_rows: EVENT_BUFFER_ROWS,
            event_buffer_bytes: EVENT_BUFFER_BYTES,
            event_buffer_age: EVENT_BUFFER_AGE,
            series_quiet_time: None,
            consolidate_chunks: CONSOLIDATE_CHUNKS,
            consolidate_bytes: CONSOLIDATE_BYTES,
            max_sorted_runs: MAX_SORTED_RUNS,
            cleanup_chunks: CLEANUP_CHUNKS,
        }
    }

    /// Tier 2: 64-bit low-end devices such as aarch64 routers, 16 MB. It is conservative
    /// and untested on that target; no build or runtime validation is claimed (§0.2).
    ///
    /// Every commit is durable, because the data lives on tmpfs, where a sync costs
    /// little. The real loss interval is the host's copy of the database to flash every 15
    /// to 60 minutes; that copy is a duty of the host (§8.3, §10.1), and nothing in this
    /// profile makes it happen.
    pub fn low_end() -> Self {
        Self {
            head: 3 * MB,
            catalog: 3 * MB,
            backend_cache: 2 * MB,
            backend_overhead: MB,
            query: 5 * MB,
            staging: 2 * MB,
            journal_bytes: 8 * MB,
            batch_bytes: 512 * KIB,
            durable_interval: None,
            durable_after_bytes: 4 * MIB,
            event_buffer_rows: EVENT_BUFFER_ROWS,
            event_buffer_bytes: EVENT_BUFFER_BYTES,
            event_buffer_age: EVENT_BUFFER_AGE,
            series_quiet_time: None,
            consolidate_chunks: CONSOLIDATE_CHUNKS,
            consolidate_bytes: CONSOLIDATE_BYTES,
            max_sorted_runs: MAX_SORTED_RUNS,
            cleanup_chunks: CLEANUP_CHUNKS,
        }
    }

    /// The total memory budget: the sum of the six memory fields (§10.1). Saturates, so
    /// absurd overrides do not overflow.
    pub fn total_memory(&self) -> u64 {
        [
            self.head,
            self.catalog,
            self.backend_cache,
            self.backend_overhead,
            self.query,
            self.staging,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }

    /// Checks that every value that has to be positive is, and that an event buffer fits
    /// in the head.
    ///
    /// The backend overhead may be zero, for a backend that has none. A zero duration for
    /// the optional ones is rejected rather than read as `None`, so each setting has one
    /// spelling.
    pub fn validate(&self) -> Result<(), ProfileError> {
        let positive = |field: &'static str, valid: bool| -> Result<(), ProfileError> {
            ensure!(valid, NotPositiveSnafu { field });
            Ok(())
        };
        positive("head", self.head > 0)?;
        positive("catalog", self.catalog > 0)?;
        positive("backend_cache", self.backend_cache > 0)?;
        positive("query", self.query > 0)?;
        positive("staging", self.staging > 0)?;
        positive("journal_bytes", self.journal_bytes > 0)?;
        positive("batch_bytes", self.batch_bytes > 0)?;
        positive(
            "durable_interval",
            self.durable_interval
                .is_none_or(|interval| !interval.is_zero()),
        )?;
        positive("durable_after_bytes", self.durable_after_bytes > 0)?;
        positive("event_buffer_rows", self.event_buffer_rows > 0)?;
        positive("event_buffer_bytes", self.event_buffer_bytes > 0)?;
        positive("event_buffer_age", !self.event_buffer_age.is_zero())?;
        positive(
            "series_quiet_time",
            self.series_quiet_time.is_none_or(|quiet| !quiet.is_zero()),
        )?;
        positive("consolidate_chunks", self.consolidate_chunks > 0)?;
        positive("consolidate_bytes", self.consolidate_bytes > 0)?;
        positive("max_sorted_runs", self.max_sorted_runs > 0)?;
        positive("cleanup_chunks", self.cleanup_chunks > 0)?;

        ensure!(
            self.event_buffer_bytes <= self.head,
            EventBufferExceedsHeadSnafu {
                buffer: self.event_buffer_bytes,
                head: self.head
            }
        );
        Ok(())
    }
}

/// A profile that [`Profile::validate`] refuses.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ProfileError {
    /// A value that has to be positive is zero.
    #[snafu(display("profile field `{field}` must be positive"))]
    NotPositive { field: &'static str },

    /// A stream's event buffer is larger than the head, so one buffer could not be held
    /// before its rows become a chunk (§4.2).
    #[snafu(display(
        "the event buffer ({buffer} bytes) does not fit in the head budget ({head} bytes)"
    ))]
    EventBufferExceedsHead { buffer: u64, head: u64 },
}

impl ProfileError {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> [(&'static str, Profile); 4] {
        [
            ("desktop", Profile::desktop()),
            ("mobile", Profile::mobile()),
            ("x86_device", Profile::x86_device()),
            ("low_end", Profile::low_end()),
        ]
    }

    #[test]
    fn every_preset_is_valid() {
        for (name, profile) in presets() {
            assert_eq!(profile.validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn the_totals_are_the_tables() {
        assert_eq!(Profile::desktop().total_memory(), 192 * MB);
        assert_eq!(Profile::mobile().total_memory(), 8 * MIB);
        assert_eq!(Profile::x86_device().total_memory(), 96 * MB);
        assert_eq!(Profile::low_end().total_memory(), 16 * MB);
    }

    #[test]
    fn the_memory_lines_are_the_tables_with_the_overhead_carved_out_of_the_cache() {
        let memory = |profile: &Profile| {
            [
                profile.head,
                profile.catalog,
                profile.backend_cache + profile.backend_overhead,
                profile.query,
                profile.staging,
            ]
        };
        assert_eq!(
            memory(&Profile::desktop()),
            [32 * MB, 32 * MB, 64 * MB, 56 * MB, 8 * MB]
        );
        assert_eq!(
            memory(&Profile::x86_device()),
            [16 * MB, 16 * MB, 32 * MB, 28 * MB, 4 * MB]
        );
        assert_eq!(
            memory(&Profile::low_end()),
            [3 * MB, 3 * MB, 3 * MB, 5 * MB, 2 * MB]
        );
        // Mobile writes 1.5 MiB; half a MiB of the cache line is the overhead.
        assert_eq!(
            memory(&Profile::mobile()),
            [
                MIB + MIB / 2,
                MIB + MIB / 2,
                MIB + MIB / 2,
                2 * MIB + MIB / 2,
                MIB
            ]
        );
        assert_eq!(
            [
                Profile::desktop().backend_overhead,
                Profile::x86_device().backend_overhead,
                Profile::mobile().backend_overhead,
                Profile::low_end().backend_overhead,
            ],
            [8 * MB, 4 * MB, 512 * KIB, MB]
        );
    }

    #[test]
    fn the_journal_follows_the_table() {
        assert_eq!(Profile::desktop().journal_bytes, 64 * MB);
        assert_eq!(Profile::mobile().journal_bytes, 4 * MIB);
        assert_eq!(Profile::x86_device().journal_bytes, 32 * MB);
        assert_eq!(Profile::low_end().journal_bytes, 8 * MB);
    }

    #[test]
    fn the_benchmark_dependent_values_start_from_the_suggestions() {
        let values = |profile: &Profile| (profile.batch_bytes, profile.durable_after_bytes);
        assert_eq!(values(&Profile::desktop()), (4 * MIB, 64 * MIB));
        assert_eq!(values(&Profile::x86_device()), (2 * MIB, 32 * MIB));
        assert_eq!(values(&Profile::mobile()), (256 * KIB, 2 * MIB));
        assert_eq!(values(&Profile::low_end()), (512 * KIB, 4 * MIB));
    }

    #[test]
    fn only_mobile_defers_durability() {
        assert_eq!(Profile::desktop().durable_interval, None);
        assert_eq!(Profile::x86_device().durable_interval, None);
        assert_eq!(Profile::low_end().durable_interval, None);
        assert_eq!(
            Profile::mobile().durable_interval,
            Some(Duration::from_secs(300))
        );
    }

    #[test]
    fn the_scheduling_limits_are_the_designs_defaults_on_every_preset() {
        for (name, profile) in presets() {
            assert_eq!(profile.event_buffer_rows, 4_096, "{name}");
            assert_eq!(profile.event_buffer_bytes, 64 * KIB, "{name}");
            assert_eq!(
                profile.event_buffer_age,
                Duration::from_secs(3600),
                "{name}"
            );
            assert_eq!(profile.series_quiet_time, None, "{name}");
            assert_eq!(profile.consolidate_chunks, 64, "{name}");
            assert_eq!(profile.consolidate_bytes, 16 * MIB, "{name}");
            assert_eq!(profile.max_sorted_runs, 8, "{name}");
            assert_eq!(profile.cleanup_chunks, 64, "{name}");
        }
    }

    #[test]
    fn every_value_that_must_be_positive_is_checked() {
        type Zero = fn(&mut Profile);
        let zeroed: [(&str, Zero); 17] = [
            ("head", |p| p.head = 0),
            ("catalog", |p| p.catalog = 0),
            ("backend_cache", |p| p.backend_cache = 0),
            ("query", |p| p.query = 0),
            ("staging", |p| p.staging = 0),
            ("journal_bytes", |p| p.journal_bytes = 0),
            ("batch_bytes", |p| p.batch_bytes = 0),
            ("durable_interval", |p| {
                p.durable_interval = Some(Duration::ZERO)
            }),
            ("durable_after_bytes", |p| p.durable_after_bytes = 0),
            ("event_buffer_rows", |p| p.event_buffer_rows = 0),
            ("event_buffer_bytes", |p| p.event_buffer_bytes = 0),
            ("event_buffer_age", |p| p.event_buffer_age = Duration::ZERO),
            ("series_quiet_time", |p| {
                p.series_quiet_time = Some(Duration::ZERO)
            }),
            ("consolidate_chunks", |p| p.consolidate_chunks = 0),
            ("consolidate_bytes", |p| p.consolidate_bytes = 0),
            ("max_sorted_runs", |p| p.max_sorted_runs = 0),
            ("cleanup_chunks", |p| p.cleanup_chunks = 0),
        ];
        for (field, zero) in zeroed {
            let mut profile = Profile::desktop();
            zero(&mut profile);
            assert_eq!(
                profile.validate(),
                Err(ProfileError::NotPositive { field }),
                "{field}"
            );
        }
    }

    #[test]
    fn the_backend_overhead_may_be_zero() {
        let profile = Profile {
            backend_overhead: 0,
            ..Profile::desktop()
        };
        assert_eq!(profile.validate(), Ok(()));
    }

    #[test]
    fn the_event_buffer_must_fit_in_the_head() {
        let at_the_limit = Profile {
            head: 64 * KIB,
            event_buffer_bytes: 64 * KIB,
            ..Profile::mobile()
        };
        assert_eq!(at_the_limit.validate(), Ok(()));

        let error = Profile {
            head: 64 * KIB - 1,
            event_buffer_bytes: 64 * KIB,
            ..Profile::mobile()
        }
        .validate()
        .unwrap_err();
        assert_eq!(
            error,
            ProfileError::EventBufferExceedsHead {
                buffer: 64 * KIB,
                head: 64 * KIB - 1
            }
        );
        assert_eq!(error.kind(), ErrorKind::Local);
    }

    #[test]
    fn the_total_saturates_instead_of_overflowing() {
        let profile = Profile {
            head: u64::MAX,
            catalog: u64::MAX,
            ..Profile::desktop()
        };
        assert_eq!(profile.total_memory(), u64::MAX);
    }
}

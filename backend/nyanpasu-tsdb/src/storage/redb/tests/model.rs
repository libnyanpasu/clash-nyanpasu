//! The label-keyed facade model over real redb transactions and the existing durable disk.

use std::sync::Arc;

use super::{FaultDisk, RedbStorage, open};
use crate::{
    Tsdb,
    lifecycle::tests::support::{Observed, Probes},
    storage::conformance::CommitOutcome,
    verification::{
        model::{Model, fixed_trace},
        profile, schema,
    },
};

/// Real redb reads, cursor pages, crashes and commit faults use the shared label oracle.
#[test]
fn redb_fixed_cross_feature_traces() {
    for chunked in [false, true] {
        let disk = FaultDisk::default();
        let mut probes = Probes::default();
        probes.schedule = disk.schedule();
        let probes = Arc::new(probes);
        let limits = profile(chunked);
        let db = Tsdb::open(
            Observed::new(open(&disk, 0), probes.clone()),
            schema(),
            limits.clone(),
        )
        .unwrap();
        let reopen = disk.clone();
        let crash = disk.clone();
        let inject = disk.clone();
        fixed_trace(Model::new(
            db,
            probes,
            limits,
            move || RedbStorage::open_with_backend(reopen.backend(), 0),
            move || crash.crash(),
            move |storage, outcome| match outcome {
                CommitOutcome::NotApplied => storage.poison_next_commit(),
                CommitOutcome::Unknown { applied } => inject.fail_commit_sync(applied),
            },
            "disk.read",
        ));
        disk.assert_fault_fired();
    }
}

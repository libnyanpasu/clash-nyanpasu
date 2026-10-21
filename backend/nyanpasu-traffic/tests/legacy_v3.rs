#[path = "legacy_v3/fixture.rs"]
mod fixture;

use fixture::NOW;
use nyanpasu_traffic::*;
use nyanpasu_tsdb::{Profile, storage::redb::RedbStorage};
use serde_json::{Value, json};
use std::path::Path;

fn open(path: &Path, profile: &Profile) -> store::Opened<RedbStorage> {
    TrafficDb::open(
        RedbStorage::open(path, profile.backend_cache).unwrap(),
        profile.clone(),
    )
    .unwrap()
}

fn capture(db: &TrafficDb<RedbStorage>, session: &Session) -> Vec<u8> {
    let mut reports = Vec::new();
    let mut pages = Vec::new();
    let mut by_keys = Vec::new();
    for request in fixture::requests() {
        reports.push(db.report(session, &request, NOW).unwrap());
        for dimension in Dimension::ALL {
            let mut cursor = None;
            let mut walked = Vec::new();
            loop {
                let page = db
                    .usage_page(
                        session,
                        &request.query,
                        dimension,
                        request.metric,
                        cursor.as_ref(),
                        7,
                        NOW,
                    )
                    .unwrap();
                cursor = page.next.clone();
                walked.push(page);
                if cursor.is_none() {
                    break;
                }
            }
            let keys: Vec<_> = walked
                .iter()
                .flat_map(|p| p.groups.iter().map(|g| g.key.clone()))
                .chain(["absent".into(), String::new()])
                .collect();
            by_keys.push(
                db.usage_by_keys(session, &request.query, dimension, &keys, NOW)
                    .unwrap(),
            );
            pages.push(walked);
        }
    }
    let mut closed = Vec::new();
    for selection in fixture::selections() {
        let mut cursor = None;
        let mut connections = Vec::new();
        loop {
            let page = db
                .closed_connections(session, &selection, cursor.as_ref(), 137, NOW)
                .unwrap();
            cursor = page.next;
            connections.extend(page.connections);
            if cursor.is_none() {
                break;
            }
        }
        closed.push(connections);
    }
    // Canonical JSON object ordering is shared with the old program; array order and all
    // result fields are preserved. Cursors in closed pages intentionally are not compared.
    // Keys are sorted explicitly: a workspace build enables serde_json's preserve_order.
    let mut output = json!({ "reports": reports, "pages": pages, "by_keys": by_keys, "closed": closed, "summary": db.summary(session).unwrap() });
    output.sort_all_objects();
    serde_json::to_vec(&output).unwrap()
}

/// Leaves out the rankings, usage pages and keyed usage of each request's filtered dimensions.
/// The old implementation applied every filter to every ranking; a ranking now ignores the
/// filter on its own dimension, which the query tests cover.
fn without_filtered_dimensions(json: &[u8]) -> Vec<u8> {
    let mut output: Value = serde_json::from_slice(json).unwrap();
    let requests = fixture::requests();
    let filtered = |i: usize, dimension: &Value| {
        requests[i]
            .query
            .filters
            .iter()
            .any(|f| serde_json::to_value(f.dimension).unwrap() == *dimension)
    };
    for (i, report) in output["reports"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        report["rankings"]
            .as_array_mut()
            .unwrap()
            .retain(|r| !filtered(i, &r["dimension"]));
    }
    let dimensions: Vec<_> = Dimension::ALL
        .iter()
        .map(|d| serde_json::to_value(d).unwrap())
        .collect();
    for key in ["pages", "by_keys"] {
        let mut index = 0;
        output[key].as_array_mut().unwrap().retain(|_| {
            let kept = !filtered(
                index / dimensions.len(),
                &dimensions[index % dimensions.len()],
            );
            index += 1;
            kept
        });
    }
    output.sort_all_objects();
    serde_json::to_vec(&output).unwrap()
}

#[test]
fn v3_reports_match_the_old_implementation_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    let new = dir.path().join("traffic.tsdb");
    fixture::write(&old);
    let mut profile = Profile::desktop();
    profile.batch_bytes = 32 * 1024;
    let (mut db, _) = open(&new, &profile);
    db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
        .unwrap();
    drop(db);
    let (db, state) = open(&new, &profile);
    let (meta, active) = state.unwrap();
    assert_eq!(active.len(), 7);
    assert_eq!(meta.instance_id.as_deref(), Some("legacy-core"));
    assert_eq!(
        meta.global_counters,
        Some(Bytes {
            upload: 12345,
            download: 98765
        })
    );
    let session = Session::restore(meta, active);
    let actual = without_filtered_dimensions(&capture(&db, &session));
    // expected.json was generated by the unmodified origin/main traffic crate, opening
    // the exact file produced by fixture::write. See legacy_v3/README.md to regenerate.
    let expected = without_filtered_dimensions(include_bytes!("legacy_v3/expected.json"));
    if actual != expected {
        let a: Value = serde_json::from_slice(&actual).unwrap();
        let e: Value = serde_json::from_slice(&expected).unwrap();
        for key in ["reports", "pages", "by_keys", "closed", "summary"] {
            assert_eq!(a[key], e[key], "oracle section: {key}");
        }
        panic!("JSON byte ordering differs");
    }
    assert_eq!(
        session
            .summary(fixture::CLOSED_COUNT as u64)
            .closed_connections,
        2048
    );
}

/// Exports the same fixture for the external origin/main oracle program.
#[test]
#[ignore = "oracle regeneration; set NYANPASU_LEGACY_FIXTURE to a fresh path"]
fn export_v3_fixture() {
    fixture::write(Path::new(
        &std::env::var_os("NYANPASU_LEGACY_FIXTURE").unwrap(),
    ));
}

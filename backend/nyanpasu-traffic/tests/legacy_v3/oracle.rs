//! Produces `expected.json` with the schema-v3 store this crate replaced. Not built here:
//! it links the traffic crate of origin/main df7d3094b. See README.md for the recipe.
mod fixture;

use fixture::NOW;
use nyanpasu_traffic::*;
use serde_json::json;
use std::{path::Path, sync::Arc};

fn rows<'a>(
    stored: &'a [(Arc<Dimensions>, Usage)],
    session: &'a Session,
    q: &TrafficQuery,
) -> Vec<Row<'a>> {
    let mut rows: Vec<_> = stored.iter().map(|(d, u)| (&**d, *u, None)).collect();
    if q.scope == TrafficScope::All {
        rows.extend(session.active_rows(q.range.tier(), q.range.start(NOW)));
    }
    filter_rows(rows, &q.filters).collect()
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let store = RedbTrafficStore::open(Path::new(&args[1])).unwrap();
    let (meta, active) = store.load().unwrap().unwrap();
    let session = Session::restore(meta, active);
    let mut reports = Vec::new();
    let mut pages = Vec::new();
    let mut by_keys = Vec::new();
    for request in fixture::requests() {
        let stored = store
            .usage(request.query.range.tier(), request.query.range.start(NOW))
            .unwrap();
        let rows = rows(&stored, &session, &request.query);
        reports.push(report(rows.iter().copied(), &request));
        for dimension in Dimension::ALL {
            let mut cursor = None;
            let mut walked = Vec::new();
            loop {
                let page = usage_page(
                    rows.iter().copied(),
                    dimension,
                    request.metric,
                    cursor.as_ref(),
                    7,
                );
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
            by_keys.push(usage_by_keys(rows.iter().copied(), dimension, &keys));
            pages.push(walked);
        }
    }
    let mut closed = Vec::new();
    for selection in fixture::selections() {
        let mut cursor = None;
        let mut connections = Vec::new();
        loop {
            let page = store
                .closed_connections(cursor.as_ref(), 137, &selection)
                .unwrap();
            cursor = page.next;
            connections.extend(page.connections);
            if cursor.is_none() {
                break;
            }
        }
        closed.push(connections);
    }
    let mut output = json!({
        "reports": reports,
        "pages": pages,
        "by_keys": by_keys,
        "closed": closed,
        "summary": session.summary(store.closed_count().unwrap()),
    });
    output.sort_all_objects();
    std::fs::write(&args[2], serde_json::to_vec(&output).unwrap()).unwrap();
}

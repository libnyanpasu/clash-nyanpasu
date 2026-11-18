//! Three-way merge of portable documents.
//!
//! A document is merged per unit, not as a whole: a portable field of the
//! application or Clash document, one profile item, one folder. Units changed
//! on one side only merge silently. A unit both sides changed to different
//! values is a conflict, settled by the policy or a recorded resolution, or
//! left open. The merge is deterministic for fixed inputs, so re-running it
//! after an interrupted cycle reproduces the same result.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use snafu::{Snafu, ensure};
use specta::Type;

use super::{
    document::{
        FOLDER_ORDER, FOLDERS, ITEM_ORDER, ITEMS, OVERRIDES, PORTABLE_REVISION, PortableDocument,
        clash_override_scope, normalize_item,
    },
    scope::{APPLICATION_SCOPES, CLASH_SCOPES, DocumentKind, scope_of},
};

/// Which side wins a unit both sides changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Each device keeps its value and the conflict stays open until resolved.
    #[default]
    Manual,
    LocalWins,
    RemoteWins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides changed the same unit to different values.
    ModifyModify,
    /// One side deleted what the other modified.
    ModifyDelete { deleted_by: ConflictSide },
    /// The remote deletes a unit this device has pinned, such as its active
    /// profile. No policy settles it in the remote's favor.
    DeleteActive,
}

/// One unit left open by a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitConflict {
    /// `<field>` or `<section>/<key>`, such as `theme_mode` or `items/<uid>`.
    pub unit: String,
    pub kind: ConflictKind,
    pub local: Option<Value>,
    pub remote: Option<Value>,
}

pub struct MergeInput<'a> {
    /// The last state both sides agreed on; `None` for a device joining a vault.
    pub base: Option<&'a PortableDocument>,
    pub local: &'a PortableDocument,
    pub remote: &'a PortableDocument,
    pub policy: ConflictPolicy,
    /// A recorded decision for one conflict, consulted before the policy.
    pub resolve: &'a dyn Fn(&UnitConflict) -> Option<ConflictSide>,
    /// Units the remote must not delete on this device.
    pub pinned: &'a BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutput {
    /// What this device should hold. An open conflict keeps the local value.
    pub local: PortableDocument,
    /// What the next commit should hold. An open conflict keeps the remote value.
    pub remote: PortableDocument,
    /// The next merge base. An open conflict keeps its base value, so a unit
    /// that did merge is not compared against a stale base while a sibling
    /// conflict stays open.
    pub baseline: PortableDocument,
    pub conflicts: Vec<UnitConflict>,
    /// Whether `local` differs from the input in a unit this build knows.
    pub local_changed: bool,
    /// Whether `remote` differs from the input.
    pub remote_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum MergeError {
    #[snafu(display("cannot merge a {local:?} projection with a {other:?} projection"))]
    KindMismatch {
        local: DocumentKind,
        other: DocumentKind,
    },
}

/// How one top-level payload key splits into units.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// The value is one unit.
    Whole,
    /// A mapping with one unit per key.
    Map,
    /// The order of the mapping `of`; merged after it and never a conflict.
    Order { of: &'static str },
}

fn shape(kind: DocumentKind, key: &str) -> Shape {
    match (kind, key) {
        (DocumentKind::Clash, OVERRIDES)
        | (DocumentKind::Profiles, ITEMS | FOLDERS | "proxy_group_layouts") => Shape::Map,
        (DocumentKind::Profiles, ITEM_ORDER) => Shape::Order { of: ITEMS },
        (DocumentKind::Profiles, FOLDER_ORDER) => Shape::Order { of: FOLDERS },
        _ => Shape::Whole,
    }
}

/// Whether this build reads and writes the unit. A unit it does not know was
/// written by a newer build: the local side cannot have changed it, so its
/// absence here must not be read as a deletion.
fn is_known(kind: DocumentKind, key: &str, inner: Option<&str>) -> bool {
    match (kind, inner) {
        (DocumentKind::Application, _) => scope_of(APPLICATION_SCOPES, key).is_some(),
        (DocumentKind::Clash, Some(inner)) if key == OVERRIDES => {
            clash_override_scope(inner).is_some()
        }
        (DocumentKind::Clash, _) => scope_of(CLASH_SCOPES, key).is_some(),
        (DocumentKind::Profiles, _) => matches!(
            key,
            ITEMS
                | ITEM_ORDER
                | FOLDERS
                | FOLDER_ORDER
                | "proxy_group_layouts"
                | "global_transforms"
                | "valid"
        ),
    }
}

fn normalize(kind: DocumentKind, key: &str, value: Option<&Value>) -> Option<Value> {
    value.map(|value| match (kind, key) {
        (DocumentKind::Profiles, ITEMS) => normalize_item(value),
        _ => value.clone(),
    })
}

/// The three results of one unit.
struct Sides {
    local: Option<Value>,
    remote: Option<Value>,
    baseline: Option<Value>,
}

impl Sides {
    fn all(value: Option<&Value>) -> Self {
        Self {
            local: value.cloned(),
            remote: value.cloned(),
            baseline: value.cloned(),
        }
    }
}

struct Merger<'a> {
    kind: DocumentKind,
    input: &'a MergeInput<'a>,
    conflicts: Vec<UnitConflict>,
    local_changed: bool,
    remote_changed: bool,
}

impl Merger<'_> {
    fn unit(
        &mut self,
        key: &str,
        inner: Option<&str>,
        base: Option<&Value>,
        local: Option<&Value>,
        remote: Option<&Value>,
    ) -> Sides {
        let known = is_known(self.kind, key, inner);
        let local = if known { local } else { base };
        let (normal_base, normal_local, normal_remote) = (
            normalize(self.kind, key, base),
            normalize(self.kind, key, local),
            normalize(self.kind, key, remote),
        );
        let unit = match inner {
            Some(inner) => format!("{key}/{inner}"),
            None => key.to_owned(),
        };
        let pinned_delete =
            remote.is_none() && local.is_some() && self.input.pinned.contains(&unit);

        let local_same = normal_local == normal_base;
        let chosen = if normal_local == normal_remote || (local_same && !pinned_delete) {
            Some(ConflictSide::Remote)
        } else if normal_remote == normal_base {
            Some(ConflictSide::Local)
        } else {
            None
        };
        let sides = match chosen {
            Some(ConflictSide::Remote) => Sides::all(remote),
            Some(ConflictSide::Local) => Sides::all(local),
            None => {
                let conflict = UnitConflict {
                    unit,
                    kind: match (pinned_delete, local, remote) {
                        (true, _, _) => ConflictKind::DeleteActive,
                        (false, None, _) => ConflictKind::ModifyDelete {
                            deleted_by: ConflictSide::Local,
                        },
                        (false, _, None) => ConflictKind::ModifyDelete {
                            deleted_by: ConflictSide::Remote,
                        },
                        (false, Some(_), Some(_)) => ConflictKind::ModifyModify,
                    },
                    local: local.cloned(),
                    remote: remote.cloned(),
                };
                let decision = (self.input.resolve)(&conflict).or(match self.input.policy {
                    ConflictPolicy::Manual => None,
                    ConflictPolicy::LocalWins => Some(ConflictSide::Local),
                    ConflictPolicy::RemoteWins => Some(ConflictSide::Remote),
                });
                match decision {
                    Some(ConflictSide::Local) => Sides::all(local),
                    Some(ConflictSide::Remote) if !pinned_delete => Sides::all(remote),
                    _ => {
                        self.conflicts.push(conflict);
                        Sides {
                            local: local.cloned(),
                            remote: remote.cloned(),
                            baseline: base.cloned(),
                        }
                    }
                }
            }
        };
        if known && normalize(self.kind, key, sides.local.as_ref()) != normal_local {
            self.local_changed = true;
        }
        if sides.remote.as_ref() != remote {
            self.remote_changed = true;
        }
        sides
    }
}

/// Merges `local` and `remote` against `base`.
///
/// # Errors
///
/// [`MergeError::KindMismatch`] when the projections are of different documents.
pub fn merge_document(input: MergeInput<'_>) -> Result<MergeOutput, MergeError> {
    let kind = input.local.document;
    for other in [Some(input.remote), input.base].into_iter().flatten() {
        ensure!(
            other.document == kind,
            KindMismatchSnafu {
                local: kind,
                other: other.document,
            }
        );
    }
    let empty = Map::new();
    let base = input.base.map_or(&empty, |document| &document.payload);
    let (local, remote) = (&input.local.payload, &input.remote.payload);
    let keys: BTreeSet<&String> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect();

    let mut merger = Merger {
        kind,
        input: &input,
        conflicts: Vec::new(),
        local_changed: false,
        remote_changed: false,
    };
    let mut out = [Map::new(), Map::new(), Map::new()];
    let insert = |out: &mut [Map<String, Value>; 3], key: &str, sides: [Option<Value>; 3]| {
        for (map, value) in out.iter_mut().zip(sides) {
            if let Some(value) = value {
                map.insert(key.to_owned(), value);
            }
        }
    };
    for key in keys.iter().copied() {
        let (unit_base, unit_local, unit_remote) = (base.get(key), local.get(key), remote.get(key));
        match shape(kind, key) {
            Shape::Order { .. } => {}
            Shape::Whole => {
                let sides = merger.unit(key, None, unit_base, unit_local, unit_remote);
                insert(&mut out, key, [sides.local, sides.remote, sides.baseline]);
            }
            Shape::Map => {
                let object = |value| as_object(value, &empty);
                let (map_base, map_local, map_remote) =
                    (object(unit_base), object(unit_local), object(unit_remote));
                let inner_keys: BTreeSet<&String> = map_base
                    .keys()
                    .chain(map_local.keys())
                    .chain(map_remote.keys())
                    .collect();
                let mut maps = [Map::new(), Map::new(), Map::new()];
                for inner in inner_keys {
                    let sides = merger.unit(
                        key,
                        Some(inner),
                        map_base.get(inner),
                        map_local.get(inner),
                        map_remote.get(inner),
                    );
                    insert(
                        &mut maps,
                        inner,
                        [sides.local, sides.remote, sides.baseline],
                    );
                }
                insert(&mut out, key, maps.map(|map| Some(Value::Object(map))));
            }
        }
    }
    for key in keys {
        let Shape::Order { of } = shape(kind, key) else {
            continue;
        };
        let sequences = [base.get(key), local.get(key), remote.get(key)].map(sequence);
        let merged = merge_order(&sequences);
        let sources = [&sequences[1], &sequences[2], &sequences[0]];
        let mut orders = [Vec::new(), Vec::new(), Vec::new()];
        for ((order, map), own) in orders.iter_mut().zip(&out).zip(sources) {
            let members = map.get(of).and_then(Value::as_object).unwrap_or(&empty);
            *order = fit_order(&merged, own, members);
        }
        if orders[0] != sequences[1] {
            merger.local_changed = true;
        }
        if orders[1] != sequences[2] {
            merger.remote_changed = true;
        }
        let values =
            orders.map(|order| Some(Value::Array(order.into_iter().map(Value::from).collect())));
        insert(&mut out, key, values);
    }

    let [local, remote, baseline] = out.map(|payload| PortableDocument {
        document: kind,
        portable_revision: PORTABLE_REVISION,
        payload,
    });
    Ok(MergeOutput {
        local,
        remote,
        baseline,
        conflicts: merger.conflicts,
        local_changed: merger.local_changed,
        remote_changed: merger.remote_changed,
    })
}

pub(super) fn as_object<'a>(
    value: Option<&'a Value>,
    empty: &'a Map<String, Value>,
) -> &'a Map<String, Value> {
    value.and_then(Value::as_object).unwrap_or(empty)
}

fn sequence(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// `[base, local, remote]` to one order. When both sides reordered, the remote
/// order wins and local-only entries follow in local order.
fn merge_order(sequences: &[Vec<String>; 3]) -> Vec<String> {
    let [base, local, remote] = sequences;
    if remote == base {
        return local.clone();
    }
    let mut merged = remote.clone();
    if local != base {
        let listed: BTreeSet<&String> = remote.iter().collect();
        merged.extend(local.iter().filter(|key| !listed.contains(key)).cloned());
    }
    merged
}

/// `merged` restricted to `members`, followed by members it omits: first in
/// this side's `own` order, then by key.
fn fit_order(merged: &[String], own: &[String], members: &Map<String, Value>) -> Vec<String> {
    let mut seen = BTreeMap::new();
    let mut order = Vec::new();
    let rest = members.keys().map(String::as_str);
    for key in merged.iter().chain(own).map(String::as_str).chain(rest) {
        if members.contains_key(key) && seen.insert(key, ()).is_none() {
            order.push(key.to_owned());
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(kind: DocumentKind, payload: Value) -> PortableDocument {
        let Value::Object(payload) = payload else {
            panic!("payload must be an object")
        };
        PortableDocument {
            document: kind,
            portable_revision: PORTABLE_REVISION,
            payload,
        }
    }

    fn app(payload: Value) -> PortableDocument {
        document(DocumentKind::Application, payload)
    }

    fn merge(
        base: Option<&PortableDocument>,
        local: &PortableDocument,
        remote: &PortableDocument,
        policy: ConflictPolicy,
    ) -> MergeOutput {
        merge_document(MergeInput {
            base,
            local,
            remote,
            policy,
            resolve: &|_| None,
            pinned: &BTreeSet::new(),
        })
        .unwrap()
    }

    #[test]
    fn changes_to_different_fields_merge_without_a_conflict() {
        let base = app(json!({"language": "en", "theme_mode": "light"}));
        let local = app(json!({"language": "ja", "theme_mode": "light"}));
        let remote = app(json!({"language": "en", "theme_mode": "dark"}));
        let out = merge(Some(&base), &local, &remote, ConflictPolicy::Manual);
        let merged = app(json!({"language": "ja", "theme_mode": "dark"}));
        assert!(out.conflicts.is_empty());
        assert_eq!(
            (&out.local, &out.remote, &out.baseline),
            (&merged, &merged, &merged)
        );
        assert!(out.local_changed && out.remote_changed);

        let again = merge(
            Some(&out.baseline),
            &out.local,
            &out.remote,
            ConflictPolicy::Manual,
        );
        assert!(
            !again.local_changed && !again.remote_changed,
            "merge is idempotent"
        );
    }

    #[test]
    fn an_open_conflict_keeps_each_side_and_advances_the_other_units() {
        let base = app(json!({"language": "en", "theme_mode": "light"}));
        let local = app(json!({"language": "ja", "theme_mode": "light"}));
        let remote = app(json!({"language": "fr", "theme_mode": "dark"}));
        let out = merge(Some(&base), &local, &remote, ConflictPolicy::Manual);
        assert_eq!(
            out.local,
            app(json!({"language": "ja", "theme_mode": "dark"}))
        );
        assert_eq!(out.remote, remote);
        assert_eq!(
            out.baseline,
            app(json!({"language": "en", "theme_mode": "dark"}))
        );
        assert_eq!(
            out.conflicts,
            [UnitConflict {
                unit: "language".to_owned(),
                kind: ConflictKind::ModifyModify,
                local: Some(json!("ja")),
                remote: Some(json!("fr")),
            }]
        );
        assert!(out.local_changed && !out.remote_changed);

        // A later one-sided change to the unit that did merge is a plain push.
        let edited = app(json!({"language": "ja", "theme_mode": "system"}));
        let next = merge(
            Some(&out.baseline),
            &edited,
            &out.remote,
            ConflictPolicy::Manual,
        );
        assert_eq!(next.conflicts.len(), 1);
        assert_eq!(next.remote.payload["theme_mode"], json!("system"));
        assert_eq!(next.remote.payload["language"], json!("fr"));
    }

    #[test]
    fn policies_and_resolutions_settle_a_conflict() {
        let base = app(json!({"language": "en"}));
        let local = app(json!({"language": "ja"}));
        let remote = app(json!({"language": "fr"}));
        let local_wins = merge(Some(&base), &local, &remote, ConflictPolicy::LocalWins);
        assert_eq!(
            (&local_wins.remote, local_wins.conflicts.len()),
            (&local, 0)
        );
        let remote_wins = merge(Some(&base), &local, &remote, ConflictPolicy::RemoteWins);
        assert_eq!(
            (&remote_wins.local, remote_wins.conflicts.len()),
            (&remote, 0)
        );

        let resolved = merge_document(MergeInput {
            base: Some(&base),
            local: &local,
            remote: &remote,
            policy: ConflictPolicy::RemoteWins,
            resolve: &|conflict| (conflict.unit == "language").then_some(ConflictSide::Local),
            pinned: &BTreeSet::new(),
        })
        .unwrap();
        assert_eq!(resolved.baseline, local, "a resolution outranks the policy");
    }

    #[test]
    fn a_field_from_a_newer_build_passes_through() {
        let base = app(json!({"language": "en", "future_field": 1}));
        let local = app(json!({"language": "ja"}));
        let remote = app(json!({"language": "en", "future_field": 2}));
        let out = merge(Some(&base), &local, &remote, ConflictPolicy::LocalWins);
        assert!(out.conflicts.is_empty());
        assert_eq!(
            out.remote,
            app(json!({"language": "ja", "future_field": 2}))
        );
        assert_eq!(out.baseline, out.remote);
        assert!(!out.local_changed, "an unknown field is not a local change");
    }

    #[test]
    fn clash_overrides_merge_per_key() {
        let clash = |mode: &str, ipv6: bool| {
            document(
                DocumentKind::Clash,
                json!({"overrides": {"mode": mode, "ipv6": ipv6}}),
            )
        };
        let out = merge(
            Some(&clash("rule", false)),
            &clash("global", false),
            &clash("rule", true),
            ConflictPolicy::Manual,
        );
        assert!(out.conflicts.is_empty());
        assert_eq!(out.local, clash("global", true));
    }

    fn catalog(items: &[(&str, &str)], order: &[&str]) -> PortableDocument {
        let items: Map<String, Value> = items
            .iter()
            .map(|(uid, name)| ((*uid).to_owned(), json!({"uid": uid, "name": name})))
            .collect();
        document(
            DocumentKind::Profiles,
            json!({"items": items, "order": order}),
        )
    }

    #[test]
    fn catalog_items_merge_by_id_and_order_never_conflicts() {
        let base = catalog(&[("a", "A"), ("b", "B")], &["a", "b"]);
        let local = catalog(&[("a", "A"), ("b", "B"), ("c", "C")], &["c", "a", "b"]);
        let remote = catalog(&[("a", "A2"), ("d", "D")], &["d", "a"]);
        let out = merge(Some(&base), &local, &remote, ConflictPolicy::Manual);
        assert!(out.conflicts.is_empty());
        assert_eq!(
            out.local,
            catalog(&[("a", "A2"), ("c", "C"), ("d", "D")], &["d", "a", "c"])
        );
        assert_eq!(out.local, out.remote);
    }

    #[test]
    fn a_local_reorder_alone_is_pushed() {
        let base = catalog(&[("a", "A"), ("b", "B")], &["a", "b"]);
        let local = catalog(&[("a", "A"), ("b", "B")], &["b", "a"]);
        let out = merge(Some(&base), &local, &base, ConflictPolicy::Manual);
        assert_eq!(out.remote, local);
        assert!(out.remote_changed && !out.local_changed);
    }

    #[test]
    fn modify_delete_is_a_conflict_and_the_order_follows_each_side() {
        let base = catalog(&[("a", "A"), ("b", "B")], &["a", "b"]);
        let local = catalog(&[("a", "A"), ("b", "B2")], &["a", "b"]);
        let remote = catalog(&[("a", "A")], &["a"]);
        let out = merge(Some(&base), &local, &remote, ConflictPolicy::Manual);
        assert_eq!(
            out.conflicts[0].kind,
            ConflictKind::ModifyDelete {
                deleted_by: ConflictSide::Remote
            }
        );
        assert_eq!(out.local, local);
        assert_eq!(out.remote, remote);
        assert_eq!(out.baseline, base);
    }

    #[test]
    fn the_remote_cannot_delete_a_pinned_unit() {
        let base = catalog(&[("a", "A"), ("b", "B")], &["a", "b"]);
        let remote = catalog(&[("a", "A")], &["a"]);
        let pinned = BTreeSet::from(["items/b".to_owned()]);
        let run = |resolve: &dyn Fn(&UnitConflict) -> Option<ConflictSide>| {
            merge_document(MergeInput {
                base: Some(&base),
                local: &base,
                remote: &remote,
                policy: ConflictPolicy::RemoteWins,
                resolve,
                pinned: &pinned,
            })
            .unwrap()
        };
        let out = run(&|_| Some(ConflictSide::Remote));
        assert_eq!(out.conflicts[0].kind, ConflictKind::DeleteActive);
        assert_eq!(out.local, base);
        assert!(!out.local_changed);

        let kept = run(&|_| Some(ConflictSide::Local));
        assert!(kept.conflicts.is_empty());
        assert_eq!(
            kept.remote, base,
            "keeping the active profile restores it remotely"
        );
    }

    #[test]
    fn joining_without_a_base_unions_and_reports_differences() {
        let local = catalog(&[("a", "A"), ("c", "C")], &["a", "c"]);
        let remote = catalog(&[("a", "A2"), ("d", "D")], &["d", "a"]);
        let out = merge(None, &local, &remote, ConflictPolicy::Manual);
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(out.conflicts[0].unit, "items/a");
        assert_eq!(
            out.local,
            catalog(&[("a", "A"), ("c", "C"), ("d", "D")], &["d", "a", "c"])
        );
        assert_eq!(
            out.remote,
            catalog(&[("a", "A2"), ("c", "C"), ("d", "D")], &["d", "a", "c"])
        );
    }

    #[test]
    fn projections_of_different_documents_do_not_merge() {
        let clash = document(DocumentKind::Clash, json!({}));
        let result = merge_document(MergeInput {
            base: None,
            local: &app(json!({})),
            remote: &clash,
            policy: ConflictPolicy::Manual,
            resolve: &|_| None,
            pinned: &BTreeSet::new(),
        });
        assert_eq!(
            result.unwrap_err(),
            MergeError::KindMismatch {
                local: DocumentKind::Application,
                other: DocumentKind::Clash,
            }
        );
    }
}

//! The pure change-detection planner.
//!
//! It compares the last agreed baseline, this device's current portable state
//! and the remote head, and decides what each side should do. It touches no
//! IO, owns no state and is deterministic, so re-running it after a crash
//! reproduces the same actions.

use std::collections::{BTreeMap, BTreeSet};

use nyanpasu_config::sync::{
    ConflictKind, ConflictPolicy, ConflictSide, MergeError, MergeInput, PortableDocument,
    UnitConflict, merge_document,
};
use nyanpasu_paths::LogicalPath;

use super::commit::{CommitItem, CommitManifest};

/// One item as one side sees it.
///
/// `sha256` is the canonical content hash; `None` means the item is absent or
/// deleted. `document` holds the decoded projection when the caller had to
/// decode it, which a document merge needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SideItem {
    pub sha256: Option<String>,
    pub document: Option<PortableDocument>,
}

impl SideItem {
    #[must_use]
    pub fn absent() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn present(sha256: impl Into<String>) -> Self {
        Self {
            sha256: Some(sha256.into()),
            document: None,
        }
    }

    #[must_use]
    pub fn document(document: PortableDocument) -> Self {
        Self {
            sha256: Some(document.object_id().as_str().to_owned()),
            document: Some(document),
        }
    }

    #[must_use]
    pub fn is_present(&self) -> bool {
        self.sha256.is_some()
    }
}

/// A device's whole portable state: one [`SideItem`] per logical path.
pub type PortableState = BTreeMap<LogicalPath, SideItem>;

/// What one item needs this cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAction {
    /// The sides already agree, or only the baseline differs.
    None,
    /// Upload the local bytes as a new object.
    Push,
    /// Download the remote object and apply it locally.
    Pull,
    /// Both sides changed; settle it per unit.
    Merge,
}

/// One planned item and the state each side should reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedItem {
    pub logical: LogicalPath,
    pub action: ItemAction,
    pub local: SideItem,
    pub remote: SideItem,
    pub baseline: SideItem,
}

/// Everything one cycle plans.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<PlannedItem>,
    pub conflicts: Vec<UnitConflict>,
}

impl Plan {
    pub fn with_action(&self, action: ItemAction) -> impl Iterator<Item = &PlannedItem> {
        self.items.iter().filter(move |item| item.action == action)
    }

    /// Whether the local side must change at all.
    #[must_use]
    pub fn changes_local(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item.action, ItemAction::Pull | ItemAction::Merge))
    }

    /// Whether the next commit differs from the remote head.
    #[must_use]
    pub fn changes_remote(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item.action, ItemAction::Push | ItemAction::Merge))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
pub enum PlanError {
    #[snafu(display("failed to merge {logical}"))]
    Merge {
        logical: LogicalPath,
        source: MergeError,
    },
}

/// The live items of a head commit.
#[must_use]
pub fn remote_state(head: Option<&CommitManifest>) -> PortableState {
    let mut state = PortableState::new();
    let Some(head) = head else {
        return state;
    };
    for (logical, item) in &head.items {
        if let CommitItem::Live(live) = item {
            state.insert(logical.clone(), SideItem::present(live.sha256.as_str()));
        }
    }
    state
}

/// Plans one cycle.
///
/// `baseline` is empty for a device joining a vault. `documents` supplies the
/// decoded projection of each item whose units may need merging; a file item
/// needs no entry and is settled as a whole.
///
/// # Errors
///
/// [`PlanError::Merge`] when two projections of different documents are compared.
pub struct PlanInput<'a> {
    pub baseline: &'a PortableState,
    pub local: &'a PortableState,
    pub remote: &'a PortableState,
    /// The local and remote projection of each document both sides hold.
    pub documents: &'a BTreeMap<LogicalPath, (PortableDocument, PortableDocument)>,
    /// The last agreed projection, the merge base.
    pub base_documents: &'a BTreeMap<LogicalPath, PortableDocument>,
    pub policy: ConflictPolicy,
    /// Units the remote must not delete on this device.
    pub pinned: &'a BTreeSet<LogicalPath>,
    /// Recorded resolutions, keyed by conflict id.
    pub resolutions: &'a BTreeMap<String, ConflictSide>,
}

pub fn plan(input: PlanInput<'_>) -> Result<Plan, PlanError> {
    let PlanInput {
        baseline,
        local,
        remote,
        documents,
        base_documents,
        policy,
        pinned,
        resolutions,
    } = input;
    let keys: BTreeSet<&LogicalPath> = baseline
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect();
    let mut plan = Plan::default();
    for logical in keys {
        let base = baseline.get(logical).cloned().unwrap_or_default();
        let local_side = local.get(logical).cloned().unwrap_or_default();
        let remote_side = remote.get(logical).cloned().unwrap_or_default();
        let (item, conflicts) = plan_item(
            logical,
            [&base, &local_side, &remote_side],
            documents.get(logical),
            base_documents.get(logical),
            policy,
            pinned.contains(logical),
            resolutions,
        )?;
        plan.conflicts.extend(conflicts);
        // A converged item transfers nothing, but its baseline still advances.
        // Compare content hashes: a side may or may not carry a decoded
        // projection, which is not part of the agreed state.
        if item.action != ItemAction::None || item.baseline.sha256 != base.sha256 {
            plan.items.push(item);
        }
    }
    Ok(plan)
}

type Planned = (PlannedItem, Vec<UnitConflict>);

fn plan_item(
    logical: &LogicalPath,
    sides: [&SideItem; 3],
    pair: Option<&(PortableDocument, PortableDocument)>,
    base_document: Option<&PortableDocument>,
    policy: ConflictPolicy,
    pinned: bool,
    resolutions: &BTreeMap<String, ConflictSide>,
) -> Result<Planned, PlanError> {
    let [base, local, remote] = sides;
    // After a successful cycle the baseline is the state both sides agree on.
    let keep = |action, baseline: &SideItem| {
        (
            PlannedItem {
                logical: logical.clone(),
                action,
                local: local.clone(),
                remote: remote.clone(),
                baseline: baseline.clone(),
            },
            Vec::new(),
        )
    };
    // A pinned unit the remote deleted is settled before the ordinary
    // one-sided cases: the local side never applies that deletion. The base
    // must have held the item, or a device joining an empty vault would read
    // its own first push as a deletion.
    if pinned && base.is_present() && local.is_present() && !remote.is_present() {
        return Ok((
            PlannedItem {
                logical: logical.clone(),
                action: ItemAction::Push,
                local: local.clone(),
                remote: local.clone(),
                baseline: local.clone(),
            },
            vec![conflict(
                logical,
                "delete_active",
                ConflictKind::DeleteActive,
                local,
                remote,
            )],
        ));
    }
    if local.sha256 == remote.sha256 {
        return Ok(keep(ItemAction::None, local));
    }
    if local.sha256 == base.sha256 {
        return Ok(keep(ItemAction::Pull, remote));
    }
    if remote.sha256 == base.sha256 {
        return Ok(keep(ItemAction::Push, local));
    }
    let Some((local_document, remote_document)) = pair else {
        // A file has no units to merge: the policy settles the whole item.
        let conflict = conflict(logical, "file", ConflictKind::ModifyModify, local, remote);
        let chosen = resolutions.get(&conflict.unit).copied().or(match policy {
            ConflictPolicy::Manual => None,
            ConflictPolicy::LocalWins => Some(ConflictSide::Local),
            ConflictPolicy::RemoteWins => Some(ConflictSide::Remote),
        });
        let (local_side, remote_side, baseline_side, conflicts) = match chosen {
            // An open conflict keeps each side and its base.
            None => (local.clone(), remote.clone(), base.clone(), vec![conflict]),
            Some(ConflictSide::Local) => (local.clone(), local.clone(), local.clone(), Vec::new()),
            Some(ConflictSide::Remote) => {
                (remote.clone(), remote.clone(), remote.clone(), Vec::new())
            }
        };
        return Ok((
            PlannedItem {
                logical: logical.clone(),
                action: ItemAction::Merge,
                local: local_side,
                remote: remote_side,
                baseline: baseline_side,
            },
            conflicts,
        ));
    };
    let output = merge_document(MergeInput {
        base: base_document,
        local: local_document,
        remote: remote_document,
        policy,
        resolve: &|unit| {
            resolutions
                .get(&format!("{logical}#{}", unit.unit))
                .copied()
        },
        pinned: &BTreeSet::new(),
    })
    .map_err(|source| PlanError::Merge {
        logical: logical.clone(),
        source,
    })?;
    let conflicts = output
        .conflicts
        .iter()
        .map(|unit| UnitConflict {
            unit: format!("{}#{}", logical, unit.unit),
            ..unit.clone()
        })
        .collect();
    Ok((
        PlannedItem {
            logical: logical.clone(),
            action: ItemAction::Merge,
            local: SideItem::document(output.local),
            remote: SideItem::document(output.remote),
            baseline: SideItem::document(output.baseline),
        },
        conflicts,
    ))
}

fn conflict(
    logical: &LogicalPath,
    unit: &str,
    kind: ConflictKind,
    local: &SideItem,
    remote: &SideItem,
) -> UnitConflict {
    UnitConflict {
        unit: format!("{logical}#{unit}"),
        kind,
        local: local.sha256.clone().map(serde_json::Value::from),
        remote: remote.sha256.clone().map(serde_json::Value::from),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sync::commit::{ChangedAt, CommitCodec, CommitDevice, DeviceId, LiveItem};
    use nyanpasu_config::sync::{DocumentKind, ObjectId};

    fn path(value: &str) -> LogicalPath {
        LogicalPath::parse(value).unwrap()
    }

    fn state(entries: &[(&str, &str)]) -> PortableState {
        entries
            .iter()
            .map(|(logical, sha)| (path(logical), SideItem::present(*sha)))
            .collect()
    }

    fn run(
        baseline: &PortableState,
        local: &PortableState,
        remote: &PortableState,
        policy: ConflictPolicy,
    ) -> Plan {
        plan(PlanInput {
            baseline,
            local,
            remote,
            documents: &BTreeMap::new(),
            base_documents: &BTreeMap::new(),
            policy,
            pinned: &BTreeSet::new(),
            resolutions: &BTreeMap::new(),
        })
        .unwrap()
    }

    #[test]
    fn an_unchanged_item_plans_nothing() {
        let sides = state(&[("profiles/a.yaml", "h1")]);
        let plan = run(&sides, &sides, &sides, ConflictPolicy::Manual);
        assert!(plan.items.is_empty());
        assert!(!plan.changes_local() && !plan.changes_remote());
    }

    #[test]
    fn a_converged_item_advances_its_baseline_without_transfer() {
        let base = state(&[("profiles/a.yaml", "h1")]);
        let changed = state(&[("profiles/a.yaml", "h2")]);
        let plan = run(&base, &changed, &changed, ConflictPolicy::Manual);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].action, ItemAction::None);
        assert_eq!(
            plan.items[0].baseline,
            changed[&path("profiles/a.yaml")],
            "the agreed state becomes the new base"
        );
        assert!(!plan.changes_local() && !plan.changes_remote());
    }

    #[test]
    fn a_local_change_pushes_and_a_remote_change_pulls() {
        let base = state(&[("profiles/a.yaml", "h1")]);
        let pushed = run(
            &base,
            &state(&[("profiles/a.yaml", "h2")]),
            &base,
            ConflictPolicy::Manual,
        );
        assert_eq!(pushed.items[0].action, ItemAction::Push);
        assert!(pushed.changes_remote() && !pushed.changes_local());

        let pulled = run(
            &base,
            &base,
            &state(&[("profiles/a.yaml", "h3")]),
            ConflictPolicy::Manual,
        );
        assert_eq!(pulled.items[0].action, ItemAction::Pull);
        assert!(pulled.changes_local() && !pulled.changes_remote());
    }

    #[test]
    fn equal_changes_on_both_sides_converge() {
        let base = state(&[("profiles/a.yaml", "h1")]);
        let changed = state(&[("profiles/a.yaml", "h2")]);
        let plan = run(&base, &changed, &changed, ConflictPolicy::Manual);
        assert!(plan.with_action(ItemAction::Merge).next().is_none());
        assert!(!plan.changes_local() && !plan.changes_remote());
    }

    #[test]
    fn a_file_changed_on_both_sides_follows_the_policy() {
        let base = state(&[("profiles/a.yaml", "h1")]);
        let local = state(&[("profiles/a.yaml", "h2")]);
        let remote = state(&[("profiles/a.yaml", "h3")]);
        let manual = run(&base, &local, &remote, ConflictPolicy::Manual);
        assert_eq!(manual.items[0].action, ItemAction::Merge);
        assert_eq!(manual.items[0].local, local[&path("profiles/a.yaml")]);
        assert_eq!(manual.items[0].remote, remote[&path("profiles/a.yaml")]);
        assert_eq!(manual.conflicts.len(), 1);

        let local_wins = run(&base, &local, &remote, ConflictPolicy::LocalWins);
        assert_eq!(local_wins.items[0].local, local[&path("profiles/a.yaml")]);
        assert_eq!(local_wins.items[0].remote, local[&path("profiles/a.yaml")]);
        assert!(local_wins.conflicts.is_empty());
    }

    #[test]
    fn joining_without_a_baseline_infers_nothing() {
        let local = state(&[("profiles/a.yaml", "h1"), ("profiles/c.yaml", "h3")]);
        let remote = state(&[("profiles/a.yaml", "h1"), ("profiles/d.yaml", "h4")]);
        let plan = run(
            &PortableState::new(),
            &local,
            &remote,
            ConflictPolicy::Manual,
        );
        let actions: Vec<_> = plan
            .items
            .iter()
            .filter(|item| item.action != ItemAction::None)
            .map(|item| (item.logical.as_str(), item.action))
            .collect();
        assert_eq!(
            actions,
            [
                ("profiles/c.yaml", ItemAction::Push),
                ("profiles/d.yaml", ItemAction::Pull),
            ]
        );
        assert!(
            plan.items
                .iter()
                .any(|item| item.logical.as_str() == "profiles/a.yaml"),
            "an item equal on both sides joins the baseline"
        );
    }

    #[test]
    fn a_pinned_item_is_never_deleted_by_the_remote() {
        let base = state(&[("profiles/active.yaml", "h1")]);
        let pinned = BTreeSet::from([path("profiles/active.yaml")]);
        let plan = plan(PlanInput {
            baseline: &base,
            local: &base,
            remote: &PortableState::new(),
            documents: &BTreeMap::new(),
            base_documents: &BTreeMap::new(),
            policy: ConflictPolicy::RemoteWins,
            pinned: &pinned,
            resolutions: &BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(plan.items[0].action, ItemAction::Push);
        assert_eq!(plan.items[0].remote, base[&path("profiles/active.yaml")]);
        assert_eq!(plan.conflicts[0].kind, ConflictKind::DeleteActive);
    }

    fn document(theme: &str) -> PortableDocument {
        PortableDocument {
            document: DocumentKind::Application,
            portable_revision: 1,
            payload: serde_json::from_value(json!({"theme_mode": theme})).unwrap(),
        }
    }

    #[test]
    fn document_items_merge_per_field() {
        let base_doc = document("light");
        let mut local_doc = document("light");
        local_doc
            .payload
            .insert("theme_mode".to_owned(), json!("dark"));
        let mut remote_doc = document("light");
        remote_doc
            .payload
            .insert("language".to_owned(), json!("ja"));
        let logical = path("app/application");
        let base = BTreeMap::from([(logical.clone(), SideItem::document(base_doc.clone()))]);
        let local = BTreeMap::from([(logical.clone(), SideItem::document(local_doc.clone()))]);
        let remote = BTreeMap::from([(logical.clone(), SideItem::document(remote_doc.clone()))]);
        let plan = plan(PlanInput {
            baseline: &base,
            local: &local,
            remote: &remote,
            documents: &BTreeMap::from([(logical.clone(), (local_doc, remote_doc))]),
            base_documents: &BTreeMap::from([(logical, base_doc)]),
            policy: ConflictPolicy::Manual,
            pinned: &BTreeSet::new(),
            resolutions: &BTreeMap::new(),
        })
        .unwrap();
        assert!(plan.conflicts.is_empty());
        let merged = plan.items[0].remote.document.as_ref().unwrap();
        assert_eq!(merged.payload["theme_mode"], json!("dark"));
        assert_eq!(merged.payload["language"], json!("ja"));
    }

    #[test]
    fn the_remote_state_lists_live_items_only() {
        let logical = path("profiles/a.yaml");
        let head = CommitManifest::new(
            3,
            None,
            Vec::new(),
            CommitDevice {
                id: DeviceId::new("x"),
                name: "device".to_owned(),
            },
            "2026-11-14T00:00:00Z".to_owned(),
            None,
            BTreeMap::from([(
                logical.clone(),
                CommitItem::Live(LiveItem {
                    codec: CommitCodec::Text,
                    sha256: ObjectId::of(b"a"),
                    size: 1,
                    portable_revision: None,
                    changed: ChangedAt {
                        generation: 3,
                        device: DeviceId::new("x"),
                    },
                }),
            )]),
        )
        .unwrap();
        assert_eq!(
            remote_state(Some(&head))[&logical].sha256.as_deref(),
            Some(ObjectId::of(b"a").as_str())
        );
    }
}

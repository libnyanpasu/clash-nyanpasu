//! Invalidation → FullCurrent → re-execute loop (spec §11) with node-key
//! anchor stability across rebuilds.

use indexmap::IndexSet;

use super::{orders::base_inputs, support::*};
use crate::{
    profile::{ProfileCategory, ProfileDependencyIndex},
    runtime::{
        executor::{ExecutionTarget, RuntimeArtifact, execute},
        invalidation::{SnapshotRebuild, invalidate_profile},
        snapshot::SnapshotNodeKey,
    },
};

#[test]
fn contributor_change_triggers_full_rebuild_with_stable_anchors() {
    let profiles = profiles_with(
        Some("clean"),
        &[],
        &[],
        vec![
            config_file_item("member", "member.yaml", &[]),
            composition_item("clean", None, &["member"], &[]),
        ],
    );
    let overrides = super::builtin::fixed_overrides();
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("clean")),
        &overrides,
        &[],
    );

    // 首次构建。
    let before = execute(
        &inputs,
        &MapContentSource::from_pairs(&[("member.yaml", "proxies:\n  - name: old\n")]),
        &FakeScriptRunner::default(),
    )
    .unwrap();

    // member 内容变更 → 失效判定（artifact 只携带物化图，stale_node_keys 的
    // 精确锚定需要调用方保留 stored 图——PR-3 事项；这里走 None 路径覆盖
    // rebuild 判定本身）。
    let index = ProfileDependencyIndex::build(&profiles);
    let invalidation = invalidate_profile(
        &pid("member"),
        ProfileCategory::Config,
        Some(&pid("clean")),
        &index,
        None,
    );
    assert_eq!(invalidation.rebuild, SnapshotRebuild::FullCurrent);
    assert!(invalidation.affected_configs.contains(&pid("clean")));

    // FullCurrent → 重跑 executor（新内容）。
    let after = execute(
        &inputs,
        &MapContentSource::from_pairs(&[("member.yaml", "proxies:\n  - name: new\n")]),
        &FakeScriptRunner::default(),
    )
    .unwrap();

    let after_json = after.final_config.to_json();
    let names: Vec<&str> = after_json["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["new"]);

    // 锚点稳定：重建前后的语义位置键集合一致（UI 可对齐）。
    let keys = |artifact: &RuntimeArtifact| -> IndexSet<SnapshotNodeKey> {
        artifact
            .graph
            .nodes
            .iter()
            .map(|node| node.key.clone())
            .collect()
    };
    assert_eq!(keys(&before), keys(&after));
    // 但配置内容确实变了。
    let before_json = before.final_config.to_json();
    assert_ne!(before_json["proxies"], after_json["proxies"]);
}

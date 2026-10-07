//! The snapshot graph keeps each node's change, not each node's config: a
//! longer pipeline of small edits must cost a fraction of one config tree.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::Arc,
};

use nyanpasu_config::runtime::{
    snapshot::{BuiltinStepKind, ConfigSnapshotsBuilder, OperatorTag, StoredConfigSnapshotsGraph},
    value::{ConfigValue, PathSegment},
};

thread_local! {
    // Per-thread counts keep concurrent tests out of each other's figures.
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

struct Counting;

fn count(delta: isize) {
    let _ = LIVE.try_with(|live| live.set(live.get() + delta));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Heap bytes that `make`'s result still holds once it returns.
fn retained<T>(make: impl FnOnce() -> T) -> (T, isize) {
    let base = LIVE.with(Cell::get);
    let value = make();
    (value, LIVE.with(Cell::get) - base)
}

fn config() -> ConfigValue {
    let rules = (0..5000)
        .map(|index| format!("DOMAIN-SUFFIX,host{index}.example.com,DIRECT"))
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({ "mode": "rule", "step": 0, "rules": rules }))
        .unwrap()
}

/// A main line whose every step changes one top-level scalar.
fn graph(root: &Arc<ConfigValue>, steps: u32) -> StoredConfigSnapshotsGraph {
    let mut builder = ConfigSnapshotsBuilder::new_root(root.clone(), OperatorTag::BareRoot);
    let mut working = root.clone();
    for step in 1..=steps {
        working = Arc::new(
            working
                .set_path(
                    &[PathSegment::Key(Arc::from("step"))],
                    ConfigValue::Number(step.into()),
                )
                .unwrap(),
        );
        builder
            .push(
                OperatorTag::BuiltinTransform {
                    selected_profile_id: None,
                    name: format!("step-{step}"),
                    step_index: step,
                },
                working.clone(),
            )
            .unwrap();
    }
    builder
        .push(
            OperatorTag::BuiltinStep {
                selected_profile_id: None,
                step: BuiltinStepKind::Finalizing,
            },
            working,
        )
        .unwrap();
    builder.build_stored().unwrap()
}

#[test]
fn pipeline_steps_cost_a_fraction_of_a_config_tree() {
    let (root, tree) = retained(|| Arc::new(config()));
    // The root keyframe is shared with `root`, so each figure is the steps' own.
    let (short, short_bytes) = retained(|| graph(&root, 4));
    let (long, long_bytes) = retained(|| graph(&root, 16));
    assert_eq!(long.nodes.len() - short.nodes.len(), 12);
    assert!(
        long_bytes - short_bytes < tree / 10,
        "12 more steps retained {} bytes; one config tree is {tree}",
        long_bytes - short_bytes
    );
}

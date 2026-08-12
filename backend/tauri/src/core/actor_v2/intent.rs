//! RuntimeIntentBuilder: a pure service deriving the portable reconcile
//! intent from an already-built runtime document. No I/O, no globals, no
//! implicit inputs — `RunType::default()`-style hidden state has no entry
//! here.
//!
//! Scope note: the snapshots → document merge itself remains the existing
//! enhance pipeline (already pure); the bridge stage composes the two. This
//! type owns the portable half: text, digest, CAS token.

use nyanpasu_core_manager::payload_digest;
use nyanpasu_ipc::api::status::RevisionIdInfo;
use nyanpasu_utils::core::CoreType;

/// The portable convergence intent: everything a `Reconcile` envelope needs
/// that is not host-resolved (binary paths stay host-side).
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeIntent {
    pub core_type: CoreType,
    /// The full runtime config document, serialized.
    pub config_text: String,
    /// [`payload_digest`] of `config_text` — the change identity the daemon
    /// verifies on receipt.
    pub digest: String,
    /// The revision the caller believes is applied; `None` on first start.
    pub expected_applied: Option<RevisionIdInfo>,
}

pub struct RuntimeIntentBuilder;

impl RuntimeIntentBuilder {
    /// Deterministic: the same document and inputs produce the same intent,
    /// digest included.
    pub fn build(
        core_type: CoreType,
        document: &serde_yaml::Mapping,
        expected_applied: Option<RevisionIdInfo>,
    ) -> Result<RuntimeIntent, serde_yaml::Error> {
        let config_text = serde_yaml::to_string(document)?;
        let digest = payload_digest(config_text.as_bytes());
        Ok(RuntimeIntent {
            core_type,
            config_text,
            digest,
            expected_applied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_utils::core::ClashCoreType;

    fn document() -> serde_yaml::Mapping {
        let mut document = serde_yaml::Mapping::new();
        document.insert(
            serde_yaml::Value::String("external-controller".into()),
            serde_yaml::Value::String("127.0.0.1:9090".into()),
        );
        document
    }

    #[test]
    fn the_same_inputs_produce_the_same_intent() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let first = RuntimeIntentBuilder::build(core_type.clone(), &document(), None).unwrap();
        let second = RuntimeIntentBuilder::build(core_type, &document(), None).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.digest, payload_digest(first.config_text.as_bytes()));
    }

    #[test]
    fn a_document_change_changes_the_digest() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let base = RuntimeIntentBuilder::build(core_type.clone(), &document(), None).unwrap();
        let mut changed = document();
        changed.insert(
            serde_yaml::Value::String("mixed-port".into()),
            serde_yaml::Value::Number(7890.into()),
        );
        let next = RuntimeIntentBuilder::build(core_type, &changed, None).unwrap();
        assert_ne!(base.digest, next.digest);
    }
}

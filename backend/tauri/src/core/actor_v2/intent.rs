//! RuntimeIntentBuilder: a pure service deriving the portable reconcile
//! intent from an already-built runtime document. No I/O, no globals, no
//! implicit inputs — `RunType::default()`-style hidden state has no entry
//! here.
//!
//! Scope note: the snapshots → document merge itself remains the existing
//! enhance pipeline (already pure); the bridge stage composes the two. This
//! type owns the portable half: the to-be-committed text and its digest.

use nyanpasu_core_manager::payload_digest;
use nyanpasu_utils::core::CoreType;

/// The portable convergence intent: everything a `Reconcile` envelope needs
/// that is not host-resolved (binary paths stay host-side).
///
/// Built once per candidate and then handed to both the advisory check and the
/// reconcile, so the two provably consume the identical to-be-committed bytes
/// (v2 T4). The CAS token is deliberately *not* here: it is read
/// authoritatively at admission time, long after these bytes exist.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeIntent {
    pub local_ipc: nyanpasu_core_manager::LocalIpcSettings,
    pub core_type: CoreType,
    /// The full runtime config document, serialized.
    pub config_text: String,
    /// [`payload_digest`] of `config_text` — the change identity the daemon
    /// verifies on receipt.
    pub digest: String,
}

pub struct RuntimeIntentBuilder;

impl RuntimeIntentBuilder {
    /// Deterministic: the same document and inputs produce the same intent,
    /// digest included.
    pub fn build(
        core_type: CoreType,
        document: &serde_yaml::Mapping,
        local_ipc: nyanpasu_core_manager::LocalIpcSettings,
    ) -> Result<RuntimeIntent, serde_yaml::Error> {
        let config_text = serde_yaml::to_string(document)?;
        let digest = payload_digest(config_text.as_bytes());
        Ok(RuntimeIntent {
            local_ipc,
            core_type,
            config_text,
            digest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_utils::core::ClashCoreType;

    fn test_settings() -> nyanpasu_core_manager::LocalIpcSettings {
        nyanpasu_core_manager::LocalIpcSettings {
            policy: nyanpasu_core_manager::LocalIpcPolicy::Prefer,
            keep_http_controller: true,
        }
    }
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
        let first =
            RuntimeIntentBuilder::build(core_type.clone(), &document(), test_settings()).unwrap();
        let second = RuntimeIntentBuilder::build(core_type, &document(), test_settings()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.digest, payload_digest(first.config_text.as_bytes()));
    }

    #[test]
    fn a_document_change_changes_the_digest() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let base =
            RuntimeIntentBuilder::build(core_type.clone(), &document(), test_settings()).unwrap();
        let mut changed = document();
        changed.insert(
            serde_yaml::Value::String("mixed-port".into()),
            serde_yaml::Value::Number(7890.into()),
        );
        let next = RuntimeIntentBuilder::build(core_type, &changed, test_settings()).unwrap();
        assert_ne!(base.digest, next.digest);
    }
}

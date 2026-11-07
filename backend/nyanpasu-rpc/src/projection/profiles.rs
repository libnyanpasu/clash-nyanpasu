//! Explicit application-boundary conversions for profiles.

use nyanpasu_core::{
    client::{
        agent_skills::CoreAgentSkill as DomainCoreAgentSkill,
        jobs::ProfileSyncStatus as DomainProfileSyncStatus,
        profiles::ProfileUrlProbe as DomainProfileUrlProbe,
    },
    profile_import::{
        ImportClassification as DomainImportClassification, ImportEvidence as DomainImportEvidence,
        ImportSignal as DomainImportSignal, ProfileImportKind as DomainProfileImportKind,
    },
    state::profiles::actor::NewProfileRequest as DomainNewProfileRequest,
};
use nyanpasu_rpc_protocol::wire;

use super::{IntoApplication, IntoWire};

impl IntoWire for DomainCoreAgentSkill {
    type Wire = wire::profiles::CoreAgentSkill;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::CoreAgentSkill {
            name: self.name,
            dir: self.dir,
            installed: self.installed,
        }
    }
}

impl IntoWire for DomainProfileUrlProbe {
    type Wire = wire::profiles::ProfileUrlProbe;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::ProfileUrlProbe {
            classification: self.classification.into_wire(),
            accepted: self
                .accepted
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            suggested_name: self.suggested_name,
            subscription: self.subscription,
            suggested_update_interval_minutes: self.suggested_update_interval_minutes,
            content: self.content,
        }
    }
}

impl IntoWire for DomainProfileSyncStatus {
    type Wire = wire::profiles::ProfileSyncStatus;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::ProfileSyncStatus {
            scheduled: self.scheduled,
            next_run_at: self.next_run_at,
            active: self.active,
            journal_degraded: self.journal_degraded,
            registration_error: self.registration_error,
            history_limit: self.history_limit,
        }
    }
}

impl IntoWire for DomainImportClassification {
    type Wire = wire::profiles::ImportClassification;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::ImportClassification {
            suggested: self.suggested.map(|value| value.into_wire()),
            evidence: self
                .evidence
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
        }
    }
}

impl IntoWire for DomainProfileImportKind {
    type Wire = wire::profiles::ProfileImportKind;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainProfileImportKind::Config => wire::profiles::ProfileImportKind::Config,
            DomainProfileImportKind::Overlay => wire::profiles::ProfileImportKind::Overlay,
            DomainProfileImportKind::JavaScript => wire::profiles::ProfileImportKind::JavaScript,
            DomainProfileImportKind::Lua => wire::profiles::ProfileImportKind::Lua,
        }
    }
}

impl IntoApplication for wire::profiles::ProfileImportKind {
    type Application = DomainProfileImportKind;

    fn into_application(self) -> Self::Application {
        match self {
            wire::profiles::ProfileImportKind::Config => DomainProfileImportKind::Config,
            wire::profiles::ProfileImportKind::Overlay => DomainProfileImportKind::Overlay,
            wire::profiles::ProfileImportKind::JavaScript => DomainProfileImportKind::JavaScript,
            wire::profiles::ProfileImportKind::Lua => DomainProfileImportKind::Lua,
        }
    }
}

impl IntoWire for DomainImportSignal {
    type Wire = wire::profiles::ImportSignal;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainImportSignal::SubscriptionQuota => {
                wire::profiles::ImportSignal::SubscriptionQuota
            }
            DomainImportSignal::ProfileTitle => wire::profiles::ImportSignal::ProfileTitle,
            DomainImportSignal::UpdateInterval => wire::profiles::ImportSignal::UpdateInterval,
            DomainImportSignal::FileExtension => wire::profiles::ImportSignal::FileExtension,
            DomainImportSignal::ContentType => wire::profiles::ImportSignal::ContentType,
            DomainImportSignal::ProxiesKey => wire::profiles::ImportSignal::ProxiesKey,
            DomainImportSignal::OverlayDirectives => {
                wire::profiles::ImportSignal::OverlayDirectives
            }
            DomainImportSignal::ScriptSyntax => wire::profiles::ImportSignal::ScriptSyntax,
        }
    }
}

impl IntoWire for DomainImportEvidence {
    type Wire = wire::profiles::ImportEvidence;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::ImportEvidence {
            kind: self.kind.into_wire(),
            signal: self.signal.into_wire(),
        }
    }
}

impl IntoWire for DomainNewProfileRequest {
    type Wire = wire::profiles::NewProfileRequest;

    fn into_wire(self) -> Self::Wire {
        wire::profiles::NewProfileRequest {
            metadata: self.metadata,
            definition: self.definition,
            folder: self.folder,
        }
    }
}

impl IntoApplication for wire::profiles::NewProfileRequest {
    type Application = DomainNewProfileRequest;

    fn into_application(self) -> Self::Application {
        DomainNewProfileRequest {
            metadata: self.metadata,
            definition: self.definition,
            folder: self.folder,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IntoWire;
    use nyanpasu_core::client::agent_skills::CoreAgentSkill;

    #[test]
    fn core_agent_skill_projection_preserves_serialization() {
        for skills in [
            vec![],
            vec![
                CoreAgentSkill {
                    name: "empty".into(),
                    dir: String::new(),
                    installed: false,
                },
                CoreAgentSkill {
                    name: "Windows".into(),
                    dir: r"C:\Users\測試\skills".into(),
                    installed: true,
                },
                CoreAgentSkill {
                    name: "Unix".into(),
                    dir: "/home/user/skills".into(),
                    installed: false,
                },
            ],
        ] {
            let expected = serde_json::to_value(&skills).unwrap();
            let projected: Vec<_> = skills.into_iter().map(IntoWire::into_wire).collect();
            assert_eq!(serde_json::to_value(projected).unwrap(), expected);
        }
    }
}

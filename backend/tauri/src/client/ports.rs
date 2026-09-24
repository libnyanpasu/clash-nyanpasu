//! Session-scoped port resolution over typed `PortStrategy` values (PR-3 T07).
//! Three distinct things, never one (v2 §6.2): a *candidate* resolution that a
//! runtime build consumes, the *receipt* of an apply the core actually
//! accepted, and the *confirmed* binding the rest of the app is allowed to
//! observe. `resolve_candidate` probes and returns; confirmed ports are derived
//! from the runtime store's current apply receipt. The fetcher's
//! `SelfProxyPortSource` reads that record: with no running instance there
//! is no endpoint to report.

use anyhow::Context as _;
use nyanpasu_config::{
    clash::config::{
        ClashConfig,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
    },
    runtime::executor::ResolvedPortBindings,
};

#[cfg(test)]
use super::runtime::RuntimeApplyReceipt;
use crate::service::profile_file::SelfProxyPortSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortsFingerprint {
    mixed: PortStrategy,
    socks: Option<PortStrategy>,
    http: Option<PortStrategy>,
    external: ExternalControllerStrategy,
}

impl PortsFingerprint {
    fn of(clash: &ClashConfig) -> Self {
        Self {
            mixed: clash.mixed_port.clone(),
            socks: clash.socks_port.clone(),
            http: clash.http_port.clone(),
            external: clash.external_controller.clone(),
        }
    }
}

/// One candidate resolution: the picks plus the port configuration they were
/// picked for. It is inert until an apply receipt promotes it, so a build that
/// is never applied — or one that is applied and then fails — leaves the
/// confirmed binding untouched (V10).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidatePortBindings {
    fingerprint: PortsFingerprint,
    bindings: ResolvedPortBindings,
}

impl CandidatePortBindings {
    pub fn bindings(&self) -> &ResolvedPortBindings {
        &self.bindings
    }
}

#[derive(Default)]
pub struct SessionPortResolver {
    runtime: super::runtime::RuntimeSnapshotStore,
}

impl SessionPortResolver {
    pub(crate) fn new(runtime: super::runtime::RuntimeSnapshotStore) -> Self {
        Self { runtime }
    }
    pub(crate) fn runtime(&self) -> super::runtime::RuntimeSnapshotStore {
        self.runtime.clone()
    }

    /// Resolves the ports a candidate runtime would bind. The baseline is the
    /// confirmed binding, never an earlier candidate: re-probing a field the
    /// running core still holds would make a Fixed strategy report its own
    /// port as occupied and move an AllowFallback one off it. Writes nothing.
    pub fn resolve_candidate(&self, clash: &ClashConfig) -> anyhow::Result<CandidatePortBindings> {
        let fingerprint = PortsFingerprint::of(clash);
        let previous = self
            .runtime
            .confirmed()
            .filter(|record| record.available)
            .map(|record| {
                let candidate = &record.receipt.ports;
                (candidate.fingerprint.clone(), candidate.bindings.clone())
            });
        if let Some((confirmed, ports)) = previous.as_ref()
            && *confirmed == fingerprint
        {
            return Ok(CandidatePortBindings {
                fingerprint,
                bindings: ports.clone(),
            });
        }

        // Re-pick only the fields whose strategy actually changed.
        let unchanged = |same: bool| -> Option<&ResolvedPortBindings> {
            match previous.as_ref() {
                Some((_, ports)) if same => Some(ports),
                _ => None,
            }
        };
        let mixed_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.mixed == fingerprint.mixed),
        ) {
            Some(ports) => ports.mixed_port,
            None => *clash
                .mixed_port
                .pick_and_try_port()
                .context("failed to resolve mixed port")?,
        };
        let port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.http == fingerprint.http),
        ) {
            Some(ports) => ports.port,
            None => clash
                .http_port
                .as_ref()
                .map(|strategy| strategy.pick_and_try_port())
                .transpose()
                .context("failed to resolve http port")?
                .map(|picked| *picked),
        };
        let socks_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.socks == fingerprint.socks),
        ) {
            Some(ports) => ports.socks_port,
            None => clash
                .socks_port
                .as_ref()
                .map(|strategy| strategy.pick_and_try_port())
                .transpose()
                .context("failed to resolve socks port")?
                .map(|picked| *picked),
        };
        // The external controller compares host and port strategy separately:
        // a host-only change must keep the session port pick (re-probing it
        // would race the running core exactly like the fields above).
        let external_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.external.port == fingerprint.external.port),
        ) {
            Some(ports) => ports
                .external_controller
                .as_deref()
                .and_then(|addr| addr.rsplit(':').next())
                .and_then(|raw| raw.parse::<u16>().ok()),
            None => None,
        };
        let external_port = match external_port {
            Some(port) => port,
            None => *clash
                .external_controller
                .port
                .pick_and_try_port()
                .context("failed to resolve external controller port")?,
        };
        let external_controller = Some(format!(
            "{}:{}",
            clash.external_controller.host, external_port
        ));

        let ports = ResolvedPortBindings {
            mixed_port,
            port,
            socks_port,
            external_controller,
        };
        Ok(CandidatePortBindings {
            fingerprint,
            bindings: ports,
        })
    }

    /// Promotes the candidate an apply actually succeeded with. The receipt is
    /// the argument because nothing weaker may move the confirmed slot: a
    /// deferred commit, a rolled-back reconcile or a lost reply all leave the
    /// old binding in place (v2 §6.2).
    #[cfg(test)]
    pub(in crate::client) fn confirm(&self, receipt: &RuntimeApplyReceipt) {
        self.runtime
            .record_confirmed_apply(None, std::sync::Arc::new(receipt.clone()));
    }

    pub fn confirmed(&self) -> Option<ResolvedPortBindings> {
        self.runtime
            .accepted_binding()
            .map(|receipt| receipt.ports.bindings.clone())
    }

    pub fn invalidate(&self) {
        self.runtime.invalidate();
    }
}

impl SelfProxyPortSource for SessionPortResolver {
    fn mixed_port(&self) -> Option<u16> {
        self.confirmed().map(|ports| ports.mixed_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::clash::config::{
        ClashConfig,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy, PortStrategyKind},
    };

    fn fixed(port: u16) -> PortStrategy {
        PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: port,
        }
    }

    /// A config pinned to two free-standing fixed ports, for the tests that
    /// only care about which binding is readable rather than about probing.
    fn pinned(mixed: u16, external: u16) -> ClashConfig {
        ClashConfig {
            mixed_port: fixed(mixed),
            external_controller: ExternalControllerStrategy {
                port: fixed(external),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// The receipt a real apply would produce for `candidate`. Only its port
    /// field matters to the resolver; the rest is what the core confirmed.
    fn receipt_for(candidate: CandidatePortBindings) -> RuntimeApplyReceipt {
        RuntimeApplyReceipt {
            revision: crate::client::runtime::tests::test_revision(),
            config_text: std::sync::Arc::from("mode: rule\n"),
            config_digest: "digest".into(),
            target_core: nyanpasu_config::application::ClashCore::Mihomo,
            core_spec: nyanpasu_core_manager::CoreSpec {
                kind: nyanpasu_core_manager::CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("fake-core"),
                version: None,
                features: Vec::new(),
            },
            host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: nyanpasu_core_manager::LocalIpcSettings {
                policy: nyanpasu_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            binding: crate::core::actor_v2::facade::AppliedConfigBinding {
                revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                    epoch: 1,
                    generation: 1,
                    source_hash: "source".into(),
                    effective_hash: "effective".into(),
                },
                host: crate::core::actor_v2::endpoint::ExecutionHost::Local,
                generation: 0,
            },
            ports: candidate,
        }
    }

    /// Resolves and confirms in one step, standing in for an apply the core
    /// accepted. Used to establish the baseline the probe rules work from.
    fn resolve_and_confirm(
        resolver: &SessionPortResolver,
        clash: &ClashConfig,
    ) -> ResolvedPortBindings {
        let candidate = resolver.resolve_candidate(clash).unwrap();
        resolver.confirm(&receipt_for(candidate.clone()));
        candidate.bindings().clone()
    }

    #[test]
    fn resolves_fixed_strategies_and_formats_external_controller() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(48231);
        clash.socks_port = Some(fixed(48232));
        clash.http_port = None;
        clash.external_controller.port = fixed(48233);
        let candidate = resolver.resolve_candidate(&clash).unwrap();
        let ports = candidate.bindings();
        assert_eq!(ports.mixed_port, 48231);
        assert_eq!(ports.socks_port, Some(48232));
        assert_eq!(ports.port, None);
        assert_eq!(
            ports.external_controller.as_deref(),
            Some("127.0.0.1:48233")
        );
        assert_eq!(
            resolver.mixed_port(),
            None,
            "a candidate nobody applied is not a listening endpoint"
        );
        resolver.confirm(&receipt_for(candidate));
        assert_eq!(resolver.mixed_port(), Some(48231));
    }

    /// V10: a resolution is inert. An apply that never happens, or one that
    /// fails afterwards, must leave the confirmed binding exactly as it was,
    /// so the system proxy never points at a candidate port.
    #[test]
    fn resolve_candidate_never_moves_the_confirmed_binding() {
        let resolver = SessionPortResolver::default();
        let clash = pinned(48331, 48332);
        let confirmed = resolve_and_confirm(&resolver, &clash);

        let mut next = clash.clone();
        next.mixed_port = fixed(48333);
        let candidate = resolver.resolve_candidate(&next).unwrap();
        assert_eq!(candidate.bindings().mixed_port, 48333);
        assert_eq!(
            resolver.confirmed(),
            Some(confirmed),
            "a failed apply leaves the previously confirmed binding in place"
        );
        assert_eq!(resolver.mixed_port(), Some(48331));
    }

    /// V11 (port half): with no instance known to be listening there is no
    /// endpoint to report -- neither before the first apply nor after a stop.
    #[test]
    fn an_unconfirmed_or_invalidated_session_reports_no_endpoint() {
        let resolver = SessionPortResolver::default();
        let clash = pinned(48431, 48432);
        assert_eq!(resolver.confirmed(), None);
        assert_eq!(resolver.mixed_port(), None);

        resolve_and_confirm(&resolver, &clash);
        assert!(resolver.confirmed().is_some());

        resolver.invalidate();
        assert_eq!(resolver.confirmed(), None);
        assert_eq!(resolver.mixed_port(), None);
    }

    #[test]
    fn random_pick_is_sticky_until_fingerprint_changes() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 0,
        };
        let first = resolve_and_confirm(&resolver, &clash);
        let second = resolver.resolve_candidate(&clash).unwrap();
        assert_eq!(
            &first,
            second.bindings(),
            "same fingerprint must reuse the session pick"
        );
        clash.socks_port = Some(fixed(48234));
        let third = resolver.resolve_candidate(&clash).unwrap();
        let third = third.bindings();
        assert_eq!(third.socks_port, Some(48234));
        assert_eq!(
            third.mixed_port, first.mixed_port,
            "unchanged mixed strategy must keep the session pick"
        );
    }

    /// Review fix regression pin (2026-07-11): re-resolving after a partial
    /// port-config change must not re-probe unchanged fields — the running
    /// core is still holding those exact ports, so a Fixed strategy would
    /// report its own port as unavailable.
    #[test]
    fn unchanged_fields_are_not_reprobed_while_core_occupies_them() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mixed = probe.local_addr().unwrap().port();
        drop(probe);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socks = probe.local_addr().unwrap().port();
        drop(probe);

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(mixed);
        let first = resolve_and_confirm(&resolver, &clash);
        assert_eq!(first.mixed_port, mixed);

        // Simulate the running core holding the mixed port, then change an
        // unrelated field.
        let _core = std::net::TcpListener::bind(("127.0.0.1", mixed)).unwrap();
        clash.socks_port = Some(fixed(socks));
        let second = resolver
            .resolve_candidate(&clash)
            .expect("unchanged mixed port must not be re-probed");
        let second = second.bindings();
        assert_eq!(second.mixed_port, mixed);
        assert_eq!(second.socks_port, Some(socks));
    }

    /// Round-2 review fix regression pin: an external-controller host-only
    /// change must keep the session port pick — only a port-strategy change
    /// re-probes.
    #[test]
    fn external_host_change_keeps_port_pick() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let ext = probe.local_addr().unwrap().port();
        drop(probe);

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.external_controller.port = fixed(ext);
        let first = resolve_and_confirm(&resolver, &clash);
        assert_eq!(
            first.external_controller.as_deref(),
            Some(format!("127.0.0.1:{ext}").as_str())
        );

        // Simulate the running core holding the external port, then change
        // only the host.
        let _core = std::net::TcpListener::bind(("127.0.0.1", ext)).unwrap();
        clash.external_controller.host = "0.0.0.0".parse().unwrap();
        let second = resolver
            .resolve_candidate(&clash)
            .expect("host-only change must not re-probe the external port");
        assert_eq!(
            second.bindings().external_controller.as_deref(),
            Some(format!("0.0.0.0:{ext}").as_str())
        );
    }

    #[test]
    fn allow_fallback_moves_off_an_occupied_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = listener.local_addr().unwrap().port();
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy::new_allow_fallback(taken);
        let ports = resolver.resolve_candidate(&clash).unwrap();
        assert_ne!(ports.bindings().mixed_port, taken);
    }
}

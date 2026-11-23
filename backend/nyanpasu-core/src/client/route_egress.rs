//! The public address the app's own requests leave from, over the route its
//! settings give them: the OS proxy, TUN, local transparent capture, or none.
//! Unlike the direct probe it never feeds the traffic cache, because behind a
//! proxy the answer is the proxy exit's, not the machine's.
use std::sync::Arc;

use nyanpasu_config::{
    application::{IpLookupProvider, NyanpasuAppConfig, StunServer},
    clash::config::ClashConfig,
};
use nyanpasu_contracts::{
    net::{EgressMethod, ProbePermission, ProbeProxy, ProbeSources, RouteEgress},
    system_proxy::OsProxyConfig,
};
use nyanpasu_state_engine::StateSnapshot;

use super::{
    NyanpasuClient, Result, locate_egress,
    traffic::{captures_local_traffic, lists_lookup, lists_stun},
};

/// How the probe's requests are sent, and the proxy they go to if one is
/// named. Several routes can apply at once; the OS proxy wins because the
/// requests are then sent to it explicitly, then TUN, then local capture.
fn egress_route(
    os_proxy: Option<OsProxyConfig>,
    clash: &ClashConfig,
) -> (EgressMethod, Option<ProbeProxy>) {
    match os_proxy.filter(|proxy| proxy.enable) {
        Some(proxy) => (
            EgressMethod::SystemProxy,
            Some(ProbeProxy {
                host: proxy.host,
                port: proxy.port,
            }),
        ),
        None if clash.enable_tun_mode => (EgressMethod::Tun, None),
        None if captures_local_traffic(clash) => (EgressMethod::TransparentProxy, None),
        None => (EgressMethod::Direct, None),
    }
}

/// A running probe stops asking once detection is off, and skips a server or
/// provider that has since left the user's list. The route does not matter.
struct SettingsPermission(StateSnapshot<NyanpasuAppConfig>);

impl ProbePermission for SettingsPermission {
    fn allows_stun(&self, server: &StunServer) -> bool {
        let config = self.0.load();
        config.state.enable_local_ip_probe && lists_stun(&config.state, server)
    }

    fn allows_lookup(&self, provider: IpLookupProvider) -> bool {
        let config = self.0.load();
        config.state.enable_local_ip_probe && lists_lookup(&config.state, provider)
    }
}

impl NyanpasuClient {
    /// A caller-triggered probe of the address the app's requests leave from,
    /// over the route its settings give them (see [`EgressMethod`]).
    pub async fn probe_route_egress(&self) -> Result<RouteEgress> {
        let application = self.inner.application.snapshot_handle();
        if !application.load().state.enable_local_ip_probe {
            return Ok(RouteEgress::Disabled);
        }

        // An unreadable OS proxy is not a reason to fail the probe: it is
        // asked over plain sockets, which is also what it is labelled as.
        let os_proxy = self
            .get_os_proxy()
            .await
            .inspect_err(|error| tracing::warn!(%error, "the system proxy could not be read"))
            .ok();
        let (method, proxy) = egress_route(os_proxy, &self.inner.clash_config.snapshot().state);

        let mut sources = ProbeSources::from_config(
            &application.load().state,
            Arc::new(SettingsPermission(application.clone())),
        );
        if let Some(proxy) = proxy {
            sources = sources.through_proxy(proxy);
        }
        let probe = &self.inner.direct_egress;
        let (ipv4, ipv6) = tokio::join!(probe.ipv4(&sources), probe.ipv6(&sources));

        // Detection may be switched off while the requests are in flight.
        if !application.load().state.enable_local_ip_probe {
            return Ok(RouteEgress::Disabled);
        }
        let index = self.inner.geo_index.subscribe().borrow().clone();
        let countries = index
            .as_deref()
            .map(|index| index as &dyn super::traffic_frames::geo::CountryLookup);
        Ok(RouteEgress::Probed {
            method,
            ipv4: ipv4.map(|answer| locate_egress(answer, countries)),
            ipv6: ipv6.map(|answer| locate_egress(answer, countries)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use nyanpasu_config::{
        application::NyanpasuAppConfigPatch, clash::config::TransparentProxyMode,
    };
    use nyanpasu_contracts::{
        net::{EgressAnswer, EgressSource, MockDirectEgressProbe},
        system_proxy::{MockOsProxyPort, OsProxyError},
    };
    use struct_patch::Patch;
    use tempfile::tempdir;

    use super::*;
    use crate::client::test_support::facade::test_client_with_egress_ports;

    fn os_proxy(enable: bool) -> OsProxyConfig {
        OsProxyConfig {
            enable,
            host: "127.0.0.1".into(),
            port: 7890,
            bypass: String::new(),
        }
    }

    #[test]
    fn the_os_proxy_wins_over_capture_and_capture_over_nothing() {
        let mut clash = ClashConfig::default();
        assert_eq!(
            egress_route(Some(os_proxy(false)), &clash),
            (EgressMethod::Direct, None)
        );
        assert_eq!(egress_route(None, &clash), (EgressMethod::Direct, None));

        clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert_eq!(
            egress_route(None, &clash),
            (EgressMethod::TransparentProxy, None)
        );

        clash.enable_tun_mode = true;
        assert_eq!(egress_route(None, &clash), (EgressMethod::Tun, None));

        assert_eq!(
            egress_route(Some(os_proxy(true)), &clash),
            (
                EgressMethod::SystemProxy,
                Some(ProbeProxy {
                    host: "127.0.0.1".into(),
                    port: 7890,
                })
            )
        );
    }

    fn answer(address: Ipv4Addr) -> EgressAnswer<Ipv4Addr> {
        EgressAnswer {
            address,
            source: EgressSource::Lookup {
                provider: IpLookupProvider::Ipify,
            },
        }
    }

    async fn enable_detection(client: &NyanpasuClient) {
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn nothing_is_asked_while_detection_is_off() {
        let dir = tempdir().unwrap();
        let mut probe = MockDirectEgressProbe::new();
        probe.expect_ipv4().never();
        probe.expect_ipv6().never();
        let client =
            test_client_with_egress_ports(&dir, Arc::new(probe), Arc::new(MockOsProxyPort::new()))
                .await;

        assert_eq!(
            client.probe_route_egress().await.unwrap(),
            RouteEgress::Disabled
        );
    }

    #[tokio::test]
    async fn an_enabled_system_proxy_is_what_the_probe_asks_through() {
        let dir = tempdir().unwrap();
        let address = Ipv4Addr::new(203, 0, 113, 7);
        let mut probe = MockDirectEgressProbe::new();
        probe
            .expect_ipv4()
            .withf(|sources| {
                sources.proxy
                    == Some(ProbeProxy {
                        host: "127.0.0.1".into(),
                        port: 7890,
                    })
            })
            .times(1)
            .returning(move |_| Some(answer(address)));
        probe.expect_ipv6().times(1).returning(|_| None);
        let mut os = MockOsProxyPort::new();
        os.expect_get().returning(|| Ok(os_proxy(true)));
        let client = test_client_with_egress_ports(&dir, Arc::new(probe), Arc::new(os)).await;
        enable_detection(&client).await;

        assert_eq!(
            client.probe_route_egress().await.unwrap(),
            RouteEgress::Probed {
                method: EgressMethod::SystemProxy,
                ipv4: Some(nyanpasu_contracts::net::EgressAddress {
                    address,
                    source: answer(address).source,
                    country: None,
                }),
                ipv6: None,
            }
        );
    }

    #[tokio::test]
    async fn tun_is_probed_over_plain_sockets_and_labelled() {
        let dir = tempdir().unwrap();
        let mut probe = MockDirectEgressProbe::new();
        probe
            .expect_ipv4()
            .withf(|sources| sources.proxy.is_none())
            .times(1)
            .returning(|_| None);
        probe.expect_ipv6().times(1).returning(|_| None);
        let mut os = MockOsProxyPort::new();
        os.expect_get().returning(|| Ok(os_proxy(false)));
        let client = test_client_with_egress_ports(&dir, Arc::new(probe), Arc::new(os)).await;
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        client.patch_clash_config(patch).await.unwrap();
        enable_detection(&client).await;

        assert_eq!(
            client.probe_route_egress().await.unwrap(),
            RouteEgress::Probed {
                method: EgressMethod::Tun,
                ipv4: None,
                ipv6: None,
            }
        );
    }

    #[tokio::test]
    async fn an_unreadable_system_proxy_is_probed_over_plain_sockets() {
        let dir = tempdir().unwrap();
        let mut probe = MockDirectEgressProbe::new();
        probe
            .expect_ipv4()
            .withf(|sources| sources.proxy.is_none())
            .times(1)
            .returning(|_| None);
        probe.expect_ipv6().times(1).returning(|_| None);
        let mut os = MockOsProxyPort::new();
        os.expect_get()
            .returning(|| Err(OsProxyError::unreadable("no desktop session")));
        let client = test_client_with_egress_ports(&dir, Arc::new(probe), Arc::new(os)).await;
        enable_detection(&client).await;

        assert_eq!(
            client.probe_route_egress().await.unwrap(),
            RouteEgress::Probed {
                method: EgressMethod::Direct,
                ipv4: None,
                ipv6: None,
            }
        );
    }

    #[tokio::test]
    async fn a_running_probe_keeps_asking_under_tun_but_stops_when_detection_is_off() {
        let dir = tempdir().unwrap();
        let permission = Arc::new(std::sync::Mutex::new(None::<Arc<dyn ProbePermission>>));
        let slot = permission.clone();
        let mut probe = MockDirectEgressProbe::new();
        probe.expect_ipv4().times(1).returning(move |sources| {
            *slot.lock().unwrap() = Some(sources.permission.clone());
            None
        });
        probe.expect_ipv6().times(1).returning(|_| None);
        let mut os = MockOsProxyPort::new();
        os.expect_get().returning(|| Ok(os_proxy(false)));
        let client = test_client_with_egress_ports(&dir, Arc::new(probe), Arc::new(os)).await;
        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(true),
                ip_lookup_providers: Some(vec![IpLookupProvider::Ipify]),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        client.probe_route_egress().await.unwrap();
        let permission = permission.lock().unwrap().clone().unwrap();
        assert!(permission.allows_lookup(IpLookupProvider::Ipify));
        assert!(!permission.allows_lookup(IpLookupProvider::Dnspod));

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        client.patch_clash_config(patch).await.unwrap();
        assert!(permission.allows_lookup(IpLookupProvider::Ipify));

        client
            .patch_app_config(NyanpasuAppConfigPatch {
                enable_local_ip_probe: Some(false),
                ..NyanpasuAppConfig::new_empty_patch()
            })
            .await
            .unwrap();
        assert!(!permission.allows_lookup(IpLookupProvider::Ipify));
    }
}

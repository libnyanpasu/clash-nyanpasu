//! Actor-owned proxy cache shared by IPC and tray adapters.
use std::{sync::Arc, time::Duration};

use crate::client::runtime::{Degradation, DegradationPhase, MutationOutcome};
use anyhow::{Context, Result};
use nyanpasu_config::clash::config::clash_strategy::ProxyChangeBreakMode;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::watch, time::Instant};

use super::{
    actor_v2::{CoreClient, api::ApiClient},
    clash::{api, proxies::Proxies},
};

struct Snapshot {
    api: ApiClient,
    proxies: Proxies,
    providers: api::ProvidersProxiesRes,
    fetched: Instant,
    fingerprint: Vec<u8>,
}

enum Message {
    Read {
        force: bool,
        reply: RpcReplyPort<Result<Arc<Snapshot>>>,
    },
    Select {
        group: String,
        name: String,
        strategy: ProxyChangeBreakMode,
        reply: RpcReplyPort<Result<MutationOutcome<()>>>,
    },
    UpdateProvider {
        name: String,
        reply: RpcReplyPort<Result<()>>,
    },
    Refresh,
    Invalidated(u64),
}

struct ProxiesActor;
struct Args {
    core: CoreClient,
    snapshots: watch::Sender<Option<Arc<Snapshot>>>,
    changes: watch::Sender<()>,
}
struct State {
    core: CoreClient,
    snapshots: watch::Sender<Option<Arc<Snapshot>>>,
    changes: watch::Sender<()>,
    cache: Option<Arc<Snapshot>>,
    generation: u64,
    monitor: Option<tokio::task::JoinHandle<()>>,
    timer: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = self.monitor.take() {
            task.abort();
        }
        if let Some(task) = self.timer.take() {
            task.abort();
        }
    }
}
impl State {
    fn clear(&mut self) {
        if let Some(task) = self.monitor.take() {
            task.abort();
        }
        if self.cache.take().is_some() {
            self.snapshots.send_replace(None);
            self.changes.send_replace(());
        }
    }

    async fn read(&mut self, actor: &ActorRef<Message>, force: bool) -> Result<Arc<Snapshot>> {
        let api = match self.core.api_client().await {
            Ok(api) => api,
            Err(error) => {
                self.clear();
                return Err(error.into());
            }
        };
        if let Some(cache) = &self.cache {
            if cache.api.same_instance(&api) {
                if !force && cache.fetched.elapsed() < Duration::from_secs(3) {
                    return Ok(cache.clone());
                }
            } else {
                self.clear();
            }
        }
        self.refresh(actor, api).await
    }

    async fn refresh(
        &mut self,
        actor: &ActorRef<Message>,
        api: ApiClient,
    ) -> Result<Arc<Snapshot>> {
        let result = async {
            let (proxies, providers) = api.proxy_snapshot().await?;
            let proxies = api::ProxiesRes {
                proxies: proxies
                    .into_iter()
                    .map(|(name, proxy)| (name.as_str().to_owned(), proxy_item(proxy)))
                    .collect(),
            };
            let providers = api::ProvidersProxiesRes {
                providers: providers
                    .into_iter()
                    .map(|(name, provider)| {
                        Ok((name.as_str().to_owned(), provider_item(provider)?))
                    })
                    .collect::<Result<_>>()?,
            };
            let proxies = Proxies::from_responses(proxies, providers.clone())?;
            let fingerprint = serde_json::to_vec(&(&proxies, &providers))?;
            anyhow::ensure!(
                !api.is_revoked(),
                "core instance retired during proxy assembly"
            );
            Ok::<_, anyhow::Error>(Arc::new(Snapshot {
                api: api.clone(),
                proxies,
                providers,
                fetched: Instant::now(),
                fingerprint,
            }))
        }
        .await;
        match result {
            Ok(snapshot) => {
                let changed = self
                    .cache
                    .as_ref()
                    .is_none_or(|old| old.fingerprint != snapshot.fingerprint);
                self.cache = Some(snapshot.clone());
                self.snapshots.send_replace(Some(snapshot.clone()));
                if changed {
                    self.changes.send_replace(());
                }
                if let Some(task) = self.monitor.take() {
                    task.abort();
                }
                self.generation += 1;
                let generation = self.generation;
                let actor = actor.clone();
                self.monitor = Some(tokio::spawn(async move {
                    api.cancelled().await;
                    let _ = actor.cast(Message::Invalidated(generation));
                }));
                Ok(snapshot)
            }
            Err(error) => {
                self.clear();
                Err(error)
            }
        }
    }

    async fn select(
        &mut self,
        actor: &ActorRef<Message>,
        group: String,
        name: String,
        strategy: ProxyChangeBreakMode,
    ) -> Result<MutationOutcome<()>> {
        self.clear();
        let api = self.core.api_client().await?;
        api.select_proxy(&group.clone().into(), &name.into())
            .await?;
        // Keep every follow-up on the selection's revocable source capability.
        let interruption = async {
            match strategy {
                ProxyChangeBreakMode::Off => {}
                ProxyChangeBreakMode::All => api.close_all_connections().await?,
                ProxyChangeBreakMode::ProxyGroup => {
                    for connection in api.connections().await?.connections.unwrap_or_default() {
                        if connection.chains.contains(&group) {
                            api.close_connection(connection.id).await?;
                        }
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let mut degradations = Vec::new();
        if let Err(error) = interruption {
            degradations.push(Degradation {
                phase: DegradationPhase::SystemEffect,
                code: "proxy_interruption_failed".into(),
                message: format!(
                    "proxy selected, but source-instance connection interruption failed: {error}"
                ),
                retryable: false,
            });
        }
        if let Err(error) = self.refresh(actor, api).await {
            degradations.push(Degradation {
                phase: DegradationPhase::UiEffect,
                code: "proxy_cache_refresh_failed".into(),
                message: format!("proxy selected, but cache refresh failed: {error}"),
                retryable: true,
            });
        }
        Ok(MutationOutcome::from_parts((), degradations))
    }
}

impl Actor for ProxiesActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;
    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            core: args.core,
            snapshots: args.snapshots,
            changes: args.changes,
            cache: None,
            generation: 0,
            monitor: None,
            timer: None,
        })
    }
    async fn post_start(
        &self,
        actor: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.timer = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if actor
                    .call(
                        |reply| Message::Read { force: true, reply },
                        Some(Duration::from_secs(120)),
                    )
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(())
    }
    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Read { force, reply } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let _ = reply.send(state.read(&actor, force).await);
            }
            Message::Select {
                group,
                name,
                strategy,
                reply,
            } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let _ = reply.send(state.select(&actor, group, name, strategy).await);
            }
            Message::UpdateProvider { name, reply } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let result = async {
                    state.clear();
                    let api = state.core.api_client().await?;
                    api.update_proxy_provider(&name.into()).await?;
                    state
                        .refresh(&actor, api)
                        .await
                        .context("provider update succeeded but cache refresh failed")?;
                    Ok(())
                }
                .await;
                let _ = reply.send(result);
            }
            Message::Refresh => {
                if let Err(error) = state.read(&actor, true).await {
                    tracing::debug!(%error, "proxy cache refresh failed");
                }
            }
            Message::Invalidated(generation) if generation == state.generation => state.clear(),
            Message::Invalidated(_) => {}
        }
        Ok(())
    }
}

struct ClientInner {
    actor: ActorRef<Message>,
    snapshots: watch::Receiver<Option<Arc<Snapshot>>>,
    changes: watch::Receiver<()>,
}
impl Drop for ClientInner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}
#[derive(Clone)]
pub(crate) struct ProxiesClient(Arc<ClientInner>);
impl ProxiesClient {
    pub async fn spawn(core: CoreClient) -> Result<Self> {
        let (snapshots, snapshot_rx) = watch::channel(None);
        let (changes, changes_rx) = watch::channel(());
        let (actor, _) = Actor::spawn(
            None,
            ProxiesActor,
            Args {
                core,
                snapshots,
                changes,
            },
        )
        .await?;
        Ok(Self(Arc::new(ClientInner {
            actor,
            snapshots: snapshot_rx,
            changes: changes_rx,
        })))
    }
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<Result<T>>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .actor
            .call(message, Some(Duration::from_secs(120)))
            .await
        {
            Ok(ractor::rpc::CallResult::Success(result)) => result,
            Ok(ractor::rpc::CallResult::Timeout) => anyhow::bail!(
                "proxy actor timed out; an operation may still be running, do not replay mutations automatically"
            ),
            _ => anyhow::bail!("proxy actor is unavailable"),
        }
    }
    pub async fn get(&self, force: bool) -> Result<Proxies> {
        let snapshot = self.call(|reply| Message::Read { force, reply }).await?;
        anyhow::ensure!(
            !snapshot.api.is_revoked(),
            "proxy snapshot belongs to a retired instance"
        );
        Ok(snapshot.proxies.clone())
    }
    pub async fn providers(&self) -> Result<api::ProvidersProxiesRes> {
        let snapshot = self
            .call(|reply| Message::Read {
                force: false,
                reply,
            })
            .await?;
        anyhow::ensure!(
            !snapshot.api.is_revoked(),
            "provider snapshot belongs to a retired instance"
        );
        Ok(snapshot.providers.clone())
    }
    pub async fn select(
        &self,
        group: String,
        name: String,
        strategy: ProxyChangeBreakMode,
    ) -> Result<MutationOutcome<()>> {
        self.call(|reply| Message::Select {
            group,
            name,
            strategy,
            reply,
        })
        .await
    }
    pub async fn update_provider(&self, name: String) -> Result<()> {
        self.call(|reply| Message::UpdateProvider { name, reply })
            .await
    }
    pub fn request_refresh(&self) {
        let _ = self.0.actor.cast(Message::Refresh);
    }
    pub fn snapshot(&self) -> Proxies {
        self.0
            .snapshots
            .borrow()
            .as_ref()
            .filter(|snapshot| !snapshot.api.is_revoked())
            .map(|snapshot| snapshot.proxies.clone())
            .unwrap_or_default()
    }
    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.0.changes.clone()
    }
}

fn proxy_item(proxy: clash_api::Proxy) -> api::ProxyItem {
    api::ProxyItem {
        name: proxy.name.as_str().to_owned(),
        r#type: proxy.proxy_type,
        udp: proxy.udp,
        history: proxy
            .history
            .into_iter()
            .map(|item| api::ProxyItemHistory {
                time: item.time.to_rfc3339(),
                delay: item.delay,
            })
            .collect(),
        all: proxy.all.map(|items| {
            items
                .into_iter()
                .map(|name| name.as_str().to_owned())
                .collect()
        }),
        now: proxy.now.map(|name| name.as_str().to_owned()),
        provider: proxy
            .provider
            .filter(|name| !name.is_empty())
            .or_else(|| proxy.provider_name.filter(|name| !name.is_empty())),
        alive: proxy.alive,
        xudp: proxy.xudp,
        tfo: proxy.tfo,
        icon: proxy.icon,
        hidden: proxy.hidden.unwrap_or(false),
    }
}
fn provider_item(provider: clash_api::ProxyProvider) -> Result<api::ProxyProviderItem> {
    Ok(api::ProxyProviderItem {
        name: provider.name.as_str().to_owned(),
        r#type: match provider.provider_type {
            clash_api::ProviderType::Proxy => api::ProviderType::Proxy,
            clash_api::ProviderType::Rule => api::ProviderType::Rule,
            clash_api::ProviderType::Unknown(value) => api::ProviderType::Unknown(value),
        },
        vehicle_type: match provider.vehicle_type {
            clash_api::VehicleType::Http => api::VehicleType::Http,
            clash_api::VehicleType::File => api::VehicleType::File,
            clash_api::VehicleType::Compatible => api::VehicleType::Compatible,
            clash_api::VehicleType::Inline => api::VehicleType::Inline,
            clash_api::VehicleType::Unknown(value) => api::VehicleType::Unknown(value),
        },
        proxies: provider.proxies.into_iter().map(proxy_item).collect(),
        updated_at: provider.updated_at.map(|date| date.to_rfc3339()),
        test_url: provider.test_url,
        expected_status: provider.expected_status,
        subscription_info: provider
            .subscription_info
            .map(|info| -> Result<_> {
                Ok(api::SubscriptionInfo {
                    upload: info.upload.try_into()?,
                    download: info.download.try_into()?,
                    total: info.total.try_into()?,
                    expire: info.expire.try_into()?,
                })
            })
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::actor_v2::api::tests::{endpoint, server};
    use axum::{
        Json, Router,
        extract::{Path, State as HttpState},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{delete, get, put},
    };
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    const GROUP: &str = "group/日本 ?#";
    const NODE: &str = "node/日本";
    const PROVIDER: &str = "provider/日本 ?#";
    #[derive(Default)]
    struct Fixture {
        reads: AtomicUsize,
        fail_reads: AtomicBool,
        fail_mutation: AtomicBool,
        fail_close: AtomicBool,
        hold_connections: AtomicBool,
        closed: Mutex<Vec<uuid::Uuid>>,
        hold_read: AtomicBool,
        hold_select: AtomicBool,
        entered: Notify,
        release: Notify,
        calls: Mutex<Vec<&'static str>>,
        selected: Mutex<String>,
    }
    async fn proxies(HttpState(f): HttpState<Arc<Fixture>>) -> Response {
        f.reads.fetch_add(1, Ordering::SeqCst);
        f.calls.lock().unwrap().push("read");
        if f.hold_read.load(Ordering::SeqCst) {
            f.entered.notify_one();
            f.release.notified().await;
        }
        if f.fail_reads.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let mut proxies = serde_json::json!({});
        for (name, kind) in [
            ("DIRECT", "Direct"),
            ("REJECT", "Reject"),
            ("GLOBAL", "Selector"),
            (GROUP, "Selector"),
        ] {
            proxies[name] = serde_json::json!({"name":name,"type":kind,"udp":true,"history":[]});
        }
        proxies["GLOBAL"]["all"] = serde_json::json!([GROUP]);
        proxies[GROUP]["all"] = serde_json::json!([NODE, "DIRECT"]);
        proxies[GROUP]["now"] = serde_json::json!(f.selected.lock().unwrap().clone());
        Json(serde_json::json!({"proxies":proxies})).into_response()
    }
    async fn providers() -> Json<serde_json::Value> {
        Json(
            serde_json::json!({"providers":{PROVIDER:{"name":PROVIDER,"type":"Proxy","vehicleType":"HTTP", "proxies":[{"name":NODE,"type":"Vless","udp":true,"history":[]}], "subscriptionInfo":{"Expire":42}}}}),
        )
    }
    async fn select(
        HttpState(f): HttpState<Arc<Fixture>>,
        Path(group): Path<String>,
        Json(body): Json<serde_json::Value>,
    ) -> StatusCode {
        assert_eq!(group, GROUP);
        assert_eq!(body["name"], NODE);
        f.calls.lock().unwrap().push("select");
        if f.hold_select.load(Ordering::SeqCst) {
            f.entered.notify_one();
            f.release.notified().await;
        }
        if f.fail_mutation.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE;
        }
        *f.selected.lock().unwrap() = NODE.into();
        StatusCode::NO_CONTENT
    }
    async fn update(HttpState(f): HttpState<Arc<Fixture>>, Path(name): Path<String>) -> StatusCode {
        assert_eq!(name, PROVIDER);
        f.calls.lock().unwrap().push("update");
        if f.fail_mutation.load(Ordering::SeqCst) {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NO_CONTENT
        }
    }
    async fn close(HttpState(f): HttpState<Arc<Fixture>>) -> StatusCode {
        f.calls.lock().unwrap().push("close");
        if f.fail_close.load(Ordering::SeqCst) {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NO_CONTENT
        }
    }
    async fn connections(HttpState(f): HttpState<Arc<Fixture>>) -> Json<serde_json::Value> {
        f.calls.lock().unwrap().push("connections");
        if f.hold_connections.load(Ordering::SeqCst) {
            f.entered.notify_one();
            f.release.notified().await;
        }
        let connections: Vec<_> = [vec![NODE, GROUP, "GLOBAL"], vec!["other"], vec![GROUP]]
            .into_iter()
            .enumerate()
            .map(|(index, chains)| {
                serde_json::json!({
                    "id": uuid::Uuid::from_u128(index as u128 + 1),
                    "upload": 0, "download": 0, "start": "2026-01-01T00:00:00Z",
                    "chains": chains, "rule": "Match", "rulePayload": ""
                })
            })
            .collect();
        Json(serde_json::json!({"downloadTotal": 0, "uploadTotal": 0, "connections": connections}))
    }
    async fn close_one(
        HttpState(f): HttpState<Arc<Fixture>>,
        Path(id): Path<uuid::Uuid>,
    ) -> StatusCode {
        f.closed.lock().unwrap().push(id);
        close(HttpState(f)).await
    }
    async fn setup() -> (
        ProxiesClient,
        CoreClient,
        Arc<crate::core::actor_v2::api::tests::Endpoint>,
        Arc<Fixture>,
        tokio::task::JoinHandle<()>,
    ) {
        let fixture = Arc::new(Fixture::default());
        let router = Router::new()
            .route("/proxies/", get(proxies))
            .route("/providers/proxies/", get(providers))
            .route("/proxies/{group}/", put(select))
            .route("/providers/proxies/{name}/", put(update))
            .route("/connections", get(connections).delete(close))
            .route("/connections/{id}", delete(close_one))
            .with_state(fixture.clone());
        let (url, server) = server(router).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let client = ProxiesClient::spawn(core.clone()).await.unwrap();
        (client, core, endpoint, fixture, server)
    }
    #[tokio::test]
    async fn cache_ttl_and_provider_metadata_are_shared() {
        let (client, _core, _, fixture, server) = setup().await;
        let proxies = client.get(false).await.unwrap();
        assert_eq!(proxies.groups[0].all[0].name, NODE);
        assert_eq!(proxies.groups[0].all[0].provider.as_deref(), Some(PROVIDER));
        assert_eq!(
            client.providers().await.unwrap().providers[PROVIDER]
                .subscription_info
                .unwrap()
                .expire,
            42
        );
        client.get(false).await.unwrap();
        assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(4)).await;
        tokio::time::resume();
        client.get(false).await.unwrap();
        assert_eq!(fixture.reads.load(Ordering::SeqCst), 2);
        client.get(false).await.unwrap();
        assert_eq!(
            fixture.reads.load(Ordering::SeqCst),
            2,
            "unchanged refresh must renew freshness"
        );
        server.abort();
    }
    #[tokio::test]
    async fn select_closes_then_refreshes_without_replaying() {
        let (client, _core, _, fixture, server) = setup().await;
        client.get(false).await.unwrap();
        fixture.calls.lock().unwrap().clear();
        client
            .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
            .await
            .unwrap();
        assert_eq!(*fixture.calls.lock().unwrap(), ["select", "close", "read"]);
        assert_eq!(client.snapshot().groups[0].now.as_deref(), Some(NODE));
        fixture.calls.lock().unwrap().clear();
        client
            .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
            .await
            .unwrap();
        assert_eq!(*fixture.calls.lock().unwrap(), ["select", "read"]);
        fixture.calls.lock().unwrap().clear();
        client.update_provider(PROVIDER.into()).await.unwrap();
        assert_eq!(*fixture.calls.lock().unwrap(), ["update", "read"]);
        server.abort();
    }
    #[tokio::test]
    async fn successful_mutation_with_failed_refresh_is_explicit_and_clears_cache() {
        let (client, _core, _, fixture, server) = setup().await;
        client.get(false).await.unwrap();
        fixture.fail_reads.store(true, Ordering::SeqCst);
        let outcome = client
            .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
            .await
            .unwrap();
        assert_eq!(outcome.degradations()[0].code, "proxy_cache_refresh_failed");
        assert!(client.snapshot().records.is_empty());
        assert_eq!(
            fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|&&call| call == "select")
                .count(),
            1
        );
        fixture.calls.lock().unwrap().clear();
        let error = client.update_provider(PROVIDER.into()).await.unwrap_err();
        assert!(error.to_string().contains("provider update succeeded"));
        assert!(client.snapshot().records.is_empty());
        assert_eq!(*fixture.calls.lock().unwrap(), ["update", "read"]);
        fixture.calls.lock().unwrap().clear();
        fixture.fail_mutation.store(true, Ordering::SeqCst);
        assert!(
            client
                .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::All)
                .await
                .is_err()
        );
        assert_eq!(*fixture.calls.lock().unwrap(), ["select"]);
        server.abort();
    }
    #[tokio::test]
    async fn group_policy_closes_only_matching_chains() {
        let (client, _core, _, fixture, server) = setup().await;
        let outcome = client
            .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::ProxyGroup)
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        assert_eq!(
            *fixture.closed.lock().unwrap(),
            [uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3)]
        );
        assert_eq!(
            *fixture.calls.lock().unwrap(),
            ["select", "connections", "close", "close", "read"]
        );
        server.abort();
    }
    #[tokio::test]
    async fn interruption_failure_preserves_selection_and_refreshes_cache() {
        for strategy in [ProxyChangeBreakMode::All, ProxyChangeBreakMode::ProxyGroup] {
            let (client, _core, _, fixture, server) = setup().await;
            fixture.fail_close.store(true, Ordering::SeqCst);
            let outcome = client
                .select(GROUP.into(), NODE.into(), strategy)
                .await
                .unwrap();
            assert_eq!(outcome.degradations().len(), 1);
            assert_eq!(outcome.degradations()[0].code, "proxy_interruption_failed");
            assert!(!outcome.degradations()[0].retryable);
            assert_eq!(*fixture.selected.lock().unwrap(), NODE);
            assert!(!client.snapshot().records.is_empty());
            server.abort();
        }
    }
    #[tokio::test]
    async fn replacement_during_chain_read_never_receives_closure_or_cache() {
        let (client, core, endpoint, fixture, server) = setup().await;
        fixture.hold_connections.store(true, Ordering::SeqCst);
        let waiting = {
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::ProxyGroup)
                    .await
            })
        };
        fixture.entered.notified().await;
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
        core.api_client().await.unwrap();
        let outcome = waiting.await.unwrap().unwrap();
        assert_eq!(outcome.degradations().len(), 2);
        assert!(fixture.closed.lock().unwrap().is_empty());
        assert_eq!(*fixture.calls.lock().unwrap(), ["select", "connections"]);
        assert!(client.snapshot().records.is_empty());
        fixture.release.notify_one();
        server.abort();
    }
    #[tokio::test]
    async fn idle_cache_is_cleared_when_its_capability_is_revoked() {
        let (client, core, endpoint, _, server) = setup().await;
        client.get(false).await.unwrap();
        let mut changes = client.subscribe();
        changes.borrow_and_update();
        endpoint.binding.send_modify(|binding| {
            binding.as_mut().unwrap().instance_id = "replacement".into();
        });
        core.api_client().await.unwrap();
        assert!(client.snapshot().records.is_empty());
        tokio::time::timeout(Duration::from_secs(3), changes.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(client.snapshot().records.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn replacement_during_read_never_publishes_the_old_response() {
        let (client, core, endpoint, fixture, server) = setup().await;
        client.get(false).await.unwrap();
        fixture.hold_read.store(true, Ordering::SeqCst);
        let waiting = {
            let client = client.clone();
            tokio::spawn(async move { client.get(true).await })
        };
        fixture.entered.notified().await;
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
        core.api_client().await.unwrap();
        assert!(waiting.await.unwrap().is_err());
        assert!(client.snapshot().records.is_empty());
        fixture.hold_read.store(false, Ordering::SeqCst);
        fixture.release.notify_one();
        client.get(false).await.unwrap();
        assert!(!client.snapshot().records.is_empty());
        server.abort();
    }
    #[tokio::test]
    async fn queued_cancelled_mutation_is_not_executed_and_shutdown_closes_subscriptions() {
        let (client, _core, _, fixture, server) = setup().await;
        fixture.hold_select.store(true, Ordering::SeqCst);
        let first = {
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
                    .await
            })
        };
        fixture.entered.notified().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        client
            .0
            .actor
            .cast(Message::Select {
                group: GROUP.into(),
                name: NODE.into(),
                strategy: ProxyChangeBreakMode::All,
                reply: tx.into(),
            })
            .unwrap();
        drop(rx);
        fixture.release.notify_one();
        first.await.unwrap().unwrap();
        client.get(false).await.unwrap(); // Acknowledges the queued message was processed.
        assert_eq!(
            fixture
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|&&call| call == "select")
                .count(),
            1
        );
        let mut changes = client.subscribe();
        changes.borrow_and_update();
        drop(client);
        assert!(
            tokio::time::timeout(Duration::from_secs(3), changes.changed())
                .await
                .unwrap()
                .is_err()
        );
        server.abort();
    }
}

//! Desktop composition adapter for the local runtime owner.
use crate::core::traffic::{ClashTrafficSource, TrafficActorArgs, TrafficClient};
use nyanpasu_core_manager::{InstanceLifecycleEvent, InstanceLifecycleSink};
use nyanpasu_traffic::*;
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub async fn start(
    directory: PathBuf,
    cancellation: CancellationToken,
) -> TrafficResult<TrafficClient> {
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| StoreError::Unavailable(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?;
    }
    #[cfg(windows)]
    nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&directory)
        .map_err(|e| StoreError::Unavailable(e.to_string()))?;
    let store =
        adapters::redb::RedbTrafficStore::open(directory.join("traffic.redb"), 32 * 1024 * 1024)
            .await?;
    TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: Arc::new(ClashTrafficSource::default()),
        store: Arc::new(store),
        clock: Arc::new(SystemClock::default()),
        cancellation,
    })
    .await
}

pub struct LifecycleSink(pub TrafficClient);
impl InstanceLifecycleSink for LifecycleSink {
    fn publish(&self, event: InstanceLifecycleEvent) {
        let result = match event {
            InstanceLifecycleEvent::Started {
                instance_id,
                observed_at_ms,
                ..
            } => {
                let time =
                    UInt(u64::try_from(observed_at_ms).expect("wall clock predates Unix epoch"));
                let instance_id = instance_id.to_string();
                self.0.notify_instance_started(
                    NewSession {
                        host: HostId("application".into()),
                        instance_id: instance_id.clone(),
                        process_started_at: None,
                        attached_at: time,
                        late_attach: false,
                    },
                    None,
                )
            }
            InstanceLifecycleEvent::Exited {
                instance_id,
                observed_at_ms,
                ..
            } => self
                .0
                .notify_instance_exited(instance_id.to_string(), observed_at_ms),
        };
        if let Err(error) = result {
            tracing::error!(%error, "traffic lifecycle delivery unavailable");
        }
    }
}

#[derive(Default)]
struct ApiBridge {
    binding: Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>,
    context_revision: Option<String>,
    api: Option<super::api::ApiClient>,
}

impl ApiBridge {
    async fn bind(
        &mut self,
        traffic: &TrafficClient,
        binding: Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>,
        context: Option<ConfigContext>,
        source: Option<Arc<dyn TrafficSource>>,
    ) -> TrafficResult<()> {
        if let Some(previous) = &self.binding {
            match traffic.detach(previous.instance_id.clone()).await {
                Ok(()) | Err(StoreError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        self.binding = None;
        self.context_revision = None;
        self.api = None;
        traffic.set_current_instance(None).await?;
        let Some(binding) = binding else {
            return Ok(());
        };
        let attached_at = UInt(
            u64::try_from(chrono::Utc::now().timestamp_millis())
                .expect("wall clock predates Unix epoch"),
        );
        // The exact local sink may already have recorded this UUID. For a
        // remote process, the existing API proves identity but not start time.
        traffic
            .instance_started(NewSession {
                host: HostId("application".into()),
                instance_id: binding.instance_id.clone(),
                process_started_at: None,
                attached_at,
                late_attach: true,
            })
            .await?;
        let endpoint = match &binding.controller {
            nyanpasu_ipc::api::status::CoreControllerInfo::Http(url) => {
                SourceEndpoint::Http(url.clone())
            }
            nyanpasu_ipc::api::status::CoreControllerInfo::UnixSocket(path) => {
                SourceEndpoint::UnixSocket(path.clone())
            }
            nyanpasu_ipc::api::status::CoreControllerInfo::NamedPipe(path) => {
                SourceEndpoint::NamedPipe(path.clone())
            }
        };
        traffic
            .update_context(binding.instance_id.clone(), context.clone())
            .await?;
        self.context_revision = context.map(|context| context.revision);
        traffic
            .controller_bound_with_source(
                SourceBinding {
                    instance_id: binding.instance_id.clone(),
                    endpoint,
                    secret: binding.secret.clone(),
                },
                source.expect("a bound API has a source"),
            )
            .await?;
        traffic
            .set_current_instance(Some(binding.instance_id.clone()))
            .await?;
        self.binding = Some(binding);
        Ok(())
    }

    async fn refresh(
        &mut self,
        core: &super::CoreClient,
        traffic: &TrafficClient,
    ) -> TrafficResult<()> {
        let before = core.status();
        let endpoint = core.connected_endpoint().await.ok();
        let binding = if let Some(endpoint) = endpoint {
            endpoint.api_connection().await.ok().flatten()
        } else {
            None
        };
        let after = core.status();
        if before.host != after.host
            || before.generation != after.generation
            || before.connectivity != after.connectivity
        {
            return self.bind(traffic, None, None, None).await;
        }
        let changed =
            self.binding != binding || self.api.as_ref().is_some_and(|api| api.is_revoked());
        let revision = after
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.revision.as_ref())
            .map(|revision| revision.effective_hash.as_str());
        let refresh_context = changed
            || self.context_revision.is_none()
            || revision != self.context_revision.as_deref();
        let context = if refresh_context {
            // Clear before awaiting the config endpoint: intervening observations
            // must not use the previous revision while its successor is unknown.
            if let Some(binding) = &binding
                && self
                    .binding
                    .as_ref()
                    .is_some_and(|old| old.instance_id == binding.instance_id)
            {
                traffic
                    .update_context(binding.instance_id.clone(), None)
                    .await?;
            }
            self.context_revision = None;
            if let Some(binding) = &binding {
                config_context(core, &binding.instance_id, revision).await
            } else {
                None
            }
        } else {
            None
        };
        if changed {
            let api = if let Some(binding) = &binding {
                let api = match core.api_client().await {
                    Ok(api) if api.matches(binding) => api,
                    _ => return self.bind(traffic, None, None, None).await,
                };
                Some(api)
            } else {
                None
            };
            let source = api
                .as_ref()
                .map(|api| Arc::new(ApiTrafficSource(api.clone())) as Arc<dyn TrafficSource>);
            self.bind(traffic, binding, context, source).await?;
            self.api = api;
        } else if refresh_context && let Some(binding) = &binding {
            traffic
                .update_context(binding.instance_id.clone(), context.clone())
                .await?;
            self.context_revision = context.map(|context| context.revision);
        }
        Ok(())
    }
}

async fn config_context(
    core: &super::CoreClient,
    instance_id: &str,
    revision: Option<&str>,
) -> Option<ConfigContext> {
    let snapshot = match core.effective_config().await {
        Ok(Some(snapshot))
            if snapshot.instance_id == instance_id
                && revision.is_none_or(|revision| revision == snapshot.revision.effective_hash) =>
        {
            snapshot
        }
        _ => return None,
    };
    let config: serde_yaml::Value = serde_yaml::from_str(&snapshot.config).ok()?;
    let rules: Vec<String> = config
        .get("rules")
        .and_then(|value| value.as_sequence())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::to_owned)
        .collect();
    Some(ConfigContext {
        revision: snapshot.revision.effective_hash,
        rules: accounting::config_context_rules(&rules),
    })
}

struct ApiTrafficSource(super::api::ApiClient);
#[async_trait::async_trait]
impl TrafficSource for ApiTrafficSource {
    async fn connect(
        &self,
        binding: SourceBinding,
        generation: UInt,
        clock: Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream> {
        if binding.instance_id != self.0.instance_id() {
            return Err(StoreError::InvalidData(
                "traffic source belongs to a different instance".into(),
            ));
        }
        let stream = self
            .0
            .connections_stream(std::time::Duration::from_secs(1))
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        Ok(Box::pin(futures_util::stream::unfold(
            stream,
            move |mut stream| {
                let id = binding.instance_id.clone();
                let clock = clock.clone();
                async move {
                    let frame =
                        tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
                            .await;
                    let observation = match frame {
                        Ok(Some(Ok(snapshot))) => crate::core::traffic::observation_from_snapshot(
                            snapshot,
                            id,
                            generation,
                            clock.as_ref(),
                        ),
                        Ok(Some(Err(error))) => Err(StoreError::Unavailable(error.to_string())),
                        Ok(None) => return None,
                        Err(_) => Err(StoreError::Unavailable(
                            "connections read deadline elapsed".into(),
                        )),
                    };
                    Some((observation, stream))
                }
            },
        )))
    }
}

#[cfg(test)]
#[path = "traffic_host_tests.rs"]
mod tests;

/// App-owned collection uses the existing core API for either execution host.
/// Only this bridge chooses the current session; losing a binding pauses it.
pub async fn api_bridge(
    core: super::CoreClient,
    traffic: TrafficClient,
    cancellation: CancellationToken,
) {
    let mut bridge = ApiBridge::default();
    let mut events = core.subscribe_events();
    let mut poll = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            _ = poll.tick() => {},
            event = events.recv() => if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; },
        }
        if let Err(error) = bridge.refresh(&core, &traffic).await {
            tracing::warn!(%error, "application traffic binding unavailable");
        }
    }
    // Root cancellation is observed by the traffic owner, whose cleanup stops
    // all sources and marks unfinished remote sessions as unknown, not exited.
}

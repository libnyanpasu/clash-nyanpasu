//! Desktop composition adapter for the local runtime owner.
use nyanpasu_core_manager::{Host, InstanceLifecycleEvent, InstanceLifecycleSink};
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
    TrafficClient::start(actor::TrafficActorArgs {
        host: HostId("local".into()),
        source: Arc::new(adapters::clash::ClashTrafficSource::default()),
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
                controller,
                ..
            } => {
                let time =
                    UInt(u64::try_from(observed_at_ms).expect("wall clock predates Unix epoch"));
                let instance_id = instance_id.to_string();
                let endpoint = match controller.host {
                    Host::Http(url) => SourceEndpoint::Http(url.to_string()),
                    Host::UnixSocket(path) => SourceEndpoint::UnixSocket(path),
                    Host::NamedPipe(path) => SourceEndpoint::NamedPipe(path),
                    _ => {
                        tracing::error!("unsupported traffic controller transport");
                        return;
                    }
                };
                self.0.notify_instance_started(
                    NewSession {
                        host: HostId("local".into()),
                        instance_id: instance_id.clone(),
                        process_started_at: None,
                        attached_at: time,
                        late_attach: false,
                    },
                    Some(SourceBinding {
                        instance_id,
                        endpoint,
                        secret: controller.secret,
                    }),
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

/// The configuration watch only supplies the latest available context. It is
/// not a complete configuration timeline and never determines session identity.
pub async fn context_bridge(
    mut subscription: nyanpasu_core_manager::ConfigCommitSubscription,
    traffic: TrafficClient,
    cancellation: CancellationToken,
) {
    let mut initial = subscription.latest();
    loop {
        let snapshot = if let Some(snapshot) = initial.take() {
            snapshot
        } else {
            tokio::select! { _ = cancellation.cancelled() => break, snapshot = subscription.changed() => match snapshot {Some(snapshot)=>snapshot,None=>break} }
        };
        let raw_rules: Vec<String> = snapshot
            .config
            .get("rules")
            .and_then(|v| v.as_sequence())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .map(str::to_owned)
            .collect();
        let rules = accounting::config_context_rules(&raw_rules);
        if let Err(error) = traffic
            .update_context(
                snapshot.instance_id.to_string(),
                Some(ConfigContext {
                    revision: snapshot.revision.effective_hash,
                    rules,
                }),
            )
            .await
        {
            tracing::warn!(%error, "traffic configuration context unavailable");
        }
    }
}

/// Selection is a latest-state slice, distinct from the ordered process events.
pub async fn selection_bridge(
    mut status: tokio::sync::watch::Receiver<nyanpasu_core_manager::CoreStatus>,
    traffic: TrafficClient,
    cancellation: CancellationToken,
) {
    loop {
        let id = status
            .borrow_and_update()
            .instance_id
            .map(|id| id.to_string());
        if let Err(error) = traffic.notify_current_instance(id) {
            tracing::warn!(%error,"traffic instance selection unavailable");
        }
        tokio::select! { _=cancellation.cancelled()=>break, result=status.changed()=>if result.is_err(){break} }
    }
}

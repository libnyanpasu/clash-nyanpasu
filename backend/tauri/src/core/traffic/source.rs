use futures_util::StreamExt;
use nyanpasu_traffic::{model::*, ports::*};
use std::{sync::Arc, time::Duration};
#[derive(Clone)]
pub struct ClashTrafficSource {
    pub interval: Duration,
    pub connect_deadline: Duration,
    pub read_deadline: Duration,
}
impl Default for ClashTrafficSource {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            connect_deadline: Duration::from_secs(10),
            read_deadline: Duration::from_secs(5),
        }
    }
}
fn failure(e: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(e.to_string())
}
#[async_trait::async_trait]
impl TrafficSource for ClashTrafficSource {
    async fn connect(
        &self,
        binding: SourceBinding,
        generation: UInt,
        clock: Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream> {
        let endpoint = match binding.endpoint {
            SourceEndpoint::Http(s) => clash_api::Host::http(s).map_err(failure)?,
            SourceEndpoint::UnixSocket(p) => clash_api::Host::unix_socket(p),
            SourceEndpoint::NamedPipe(p) => clash_api::Host::named_pipe(p),
        };
        let mut builder = clash_api::Client::builder(endpoint);
        if let Some(secret) = binding.secret {
            builder = builder.secret(secret)
        }
        let client = builder.build().map_err(failure)?;
        let query = clash_api::ConnectionStreamQuery::new(self.interval).map_err(failure)?;
        let stream = tokio::time::timeout(self.connect_deadline, client.connections_ws(query))
            .await
            .map_err(failure)?
            .map_err(failure)?;
        let stream = read_deadline(
            Box::pin(stream.map(|frame| frame.map_err(failure))),
            self.read_deadline,
        );
        Ok(Box::pin(stream.map(move |frame| {
            frame.and_then(|snapshot| {
                observation_from_snapshot(
                    snapshot,
                    binding.instance_id.clone(),
                    generation,
                    clock.as_ref(),
                )
            })
        })))
    }
}

pub(crate) fn observation_from_snapshot(
    snapshot: clash_api::ConnectionsSnapshot,
    instance_id: String,
    generation: UInt,
    clock: &dyn Clock,
) -> TrafficResult<Observation> {
    let wall_time = clock.wall_time();
    let monotonic_ns = clock.monotonic_ns();
    let mut connections = Vec::new();
    for c in snapshot.connections.unwrap_or_default() {
        let mut metadata = std::collections::BTreeMap::new();
        if let Some(m) = c.metadata {
            let value = serde_json::to_value(m.known).map_err(failure)?;
            if let Some(fields) = value.as_object() {
                metadata.extend(fields.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            metadata.extend(m.extra);
        }
        connections.push(ConnectionSample {
            id: c.id.to_string(),
            started_at: Some(c.start.to_rfc3339()),
            metadata,
            extra: c.extra.into_iter().collect(),
            upload: c.upload,
            download: c.download,
            rule: c.rule,
            rule_payload: c.rule_payload,
            chains: c.chains,
            provider_chains: c.provider_chains.unwrap_or_default(),
        });
    }
    Ok(Observation {
        instance_id,
        generation,
        wall_time,
        monotonic_ns,
        upload_total: snapshot.upload_total,
        download_total: snapshot.download_total,
        connections,
    })
}

fn read_deadline<T: Send + 'static>(
    stream: std::pin::Pin<Box<dyn futures_util::Stream<Item = TrafficResult<T>> + Send>>,
    deadline: Duration,
) -> impl futures_util::Stream<Item = TrafficResult<T>> + Send {
    futures_util::stream::unfold((stream, false), move |(mut stream, ended)| async move {
        if ended {
            return None;
        }
        match tokio::time::timeout(deadline, stream.next()).await {
            Ok(Some(frame)) => Some((frame, (stream, false))),
            Ok(None) => None,
            Err(_) => Some((
                Err(StoreError::Unavailable(
                    "connections read deadline elapsed".into(),
                )),
                (stream, true),
            )),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn stalled_transport_has_a_network_read_deadline() {
        let stream = read_deadline::<Observation>(
            Box::pin(futures_util::stream::pending()),
            Duration::from_secs(5),
        );
        tokio::pin!(stream);
        assert!(matches!(
            stream.next().await,
            Some(Err(StoreError::Unavailable(_)))
        ));
        assert!(stream.next().await.is_none());
    }
}

//! UI projections of application-owned subscriptions; accounting remains in the application traffic owner.
use crate::client::{NyanpasuClient, traffic::TrafficStream};
use futures_util::StreamExt;
use nyanpasu_traffic::{TrafficDetails, TrafficSummary};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct TrafficSummaryFrame {
    pub summary: Option<TrafficSummary>,
    pub error: Option<String>,
}

pub fn summary_stream(client: NyanpasuClient) -> TrafficStream<TrafficSummary> {
    Box::pin(
        futures_util::stream::once(async move { client.subscribe_traffic_summary().await })
            .flat_map(|result| match result {
                Ok(stream) => stream,
                Err(error) => Box::pin(futures_util::stream::once(async move { Err(error) }))
                    as TrafficStream<TrafficSummary>,
            }),
    )
}

pub fn details_stream(client: NyanpasuClient) -> TrafficStream<TrafficDetails> {
    Box::pin(
        futures_util::stream::once(async move { client.subscribe_traffic_details().await })
            .flat_map(|result| match result {
                Ok(stream) => stream,
                Err(error) => Box::pin(futures_util::stream::once(async move { Err(error) }))
                    as TrafficStream<TrafficDetails>,
            }),
    )
}

pub async fn forward_summary(
    client: NyanpasuClient,
    sink: tauri::ipc::Channel<TrafficSummaryFrame>,
) {
    let mut stream = summary_stream(client);
    while let Some(frame) = stream.next().await {
        let frame = match frame {
            Ok(summary) => TrafficSummaryFrame {
                summary,
                error: None,
            },
            Err(error) => TrafficSummaryFrame {
                summary: None,
                error: Some(error.to_string()),
            },
        };
        if sink.send(frame).is_err() {
            break;
        }
    }
}

//! UI projections and reconnecting subscriptions; accounting remains in the host traffic owner.
use crate::{client::NyanpasuClient, core::actor_v2::endpoint::TrafficStream};
use futures_util::{StreamExt, future::BoxFuture};
use nyanpasu_traffic::{TrafficDetails, TrafficResult, TrafficSummary};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct TrafficSummaryFrame {
    pub summary: Option<TrafficSummary>,
    pub error: Option<String>,
}

pub fn summary_stream(client: NyanpasuClient) -> TrafficStream<TrafficSummary> {
    reconnecting(client, |client| {
        Box::pin(async move { client.subscribe_traffic_summary().await })
    })
}
pub fn details_stream(client: NyanpasuClient) -> TrafficStream<TrafficDetails> {
    reconnecting(client, |client| {
        Box::pin(async move { client.subscribe_traffic_details().await })
    })
}

fn reconnecting<T: Send + 'static>(
    client: NyanpasuClient,
    subscribe: fn(NyanpasuClient) -> BoxFuture<'static, TrafficResult<TrafficStream<T>>>,
) -> TrafficStream<T> {
    let events = client.subscribe_core_events();
    let status = client.core_status();
    futures_util::stream::unfold((client,events,status,None::<TrafficStream<T>>,false),move |(client,mut events,mut status,mut stream,retry)|async move{
        if retry {tokio::time::sleep(std::time::Duration::from_secs(1)).await;}
        loop {
            if stream.is_none() {
                match subscribe(client.clone()).await {
                    Ok(next)=>stream=Some(next),
                    Err(error)=>return Some((Err(error),(client,events,status,None,true))),
                }
            }
            let latest=client.core_status();
            if latest.host!=status.host||latest.generation!=status.generation||latest.connectivity!=status.connectivity {
                return Some((Ok(None),(client,events,latest,None,false)));
            }
            tokio::select! {
                next=stream.as_mut().expect("subscription established").next()=>{
                    let latest=client.core_status();
                    if latest.host!=status.host||latest.generation!=status.generation||latest.connectivity!=status.connectivity {return Some((Ok(None),(client,events,latest,None,false)));}
                    match next {
                    Some(Ok(frame))=>return Some((Ok(frame),(client,events,status,stream,false))),
                    Some(Err(error))=>return Some((Err(error),(client,events,status,None,true))),
                    None=>return Some((Ok(None),(client,events,status,None,true))),
                    }
                },
                event=events.recv()=>{
                    let next=match event {Ok(status)=>status,Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>client.core_status(),Err(tokio::sync::broadcast::error::RecvError::Closed)=>return None};
                    if next.host!=status.host || next.generation!=status.generation || next.connectivity!=status.connectivity {
                        status=next;
                        return Some((Ok(None),(client,events,status,None,false)));
                    }
                    status=next;
                },
            }
        }
    }).boxed()
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

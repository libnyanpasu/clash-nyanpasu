//! HTTP E1 observes request futures and response preparation, not client receipt.

use std::{sync::Arc, task::Poll};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures::StreamExt as _;
use nyanpasu_core::storage::WebStorage as _;
use nyanpasu_rpc_protocol::{RpcError, RpcOwner};
use serde_json::Value;
use tower::ServiceExt as _;

use crate::{
    audit::{AuditPhase, CompletionSummary, DeliveryOutcome, ObservationEnd, RpcAuditOutcome},
    registry::{RpcDependencies, RpcFuture},
    test_support::Fixture,
};

#[tokio::test]
async fn http_departure_reports_unobserved_outcome_under_e1() {
    fn pending(deps: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> Result<RpcFuture, RpcError> {
        Ok(Box::pin(crate::registry::unreported(async move {
            // The business owner starts work before this waiter goes away.
            let mut release = deps.events.subscribe();
            let storage = deps.storage.clone();
            tokio::spawn(async move {
                while release.recv().await.unwrap().0 != "release" {}
                tokio::task::spawn_blocking(move || {
                    storage.set_item("owner-completed", &"committed").unwrap()
                })
                .await
                .unwrap();
                deps.events.publish("committed", Value::Null);
            });
            std::future::pending::<Result<Value, RpcError>>().await
        })))
    }
    let mut fixture = Fixture::new(false).await;
    Arc::make_mut(&mut fixture.rpc.commands)
        .get_mut("audit_echo")
        .unwrap()
        .handler = Some(pending);
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let mut request = Box::pin(fixture.call("audit_echo", Value::Null));
    assert!(matches!(futures::poll!(&mut request), Poll::Pending));
    assert_eq!(fixture.sink.records.lock()[0].phase, AuditPhase::Admitted);
    drop(request);
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[1].phase,
            AuditPhase::Finished {
                outcome: RpcAuditOutcome::NotObserved,
                completion: CompletionSummary::NotReported,
            }
        );
        assert_eq!(
            records[2].phase,
            AuditPhase::ObservationEnded {
                reason: ObservationEnd::FutureDropped
            }
        );
        assert!(
            records
                .iter()
                .all(|record| record.header.invocation_id == records[0].header.invocation_id)
        );
    }
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    while events.recv().await.unwrap().0 != "committed" {}
    assert_eq!(
        fixture
            .rpc
            .dependencies
            .storage
            .get_item::<String>("owner-completed")
            .unwrap()
            .value,
        Some("committed".into())
    );
    assert_eq!(
        fixture.sink.records.lock().len(),
        3,
        "no executor observes the late outcome"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn archive_response_creation_is_not_download_completion() {
    let fixture = Fixture::new(false).await;
    let response = fixture
        .rpc
        .clone()
        .router()
        .oneshot(
            Request::builder()
                .uri("/bridge/logs/archive")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[2].phase,
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::ResponsePrepared
            }
        );
        assert!(matches!(
            records[1].phase,
            AuditPhase::Finished {
                outcome: RpcAuditOutcome::Success,
                ..
            }
        ));
    }
    let mut body = response.into_body().into_data_stream();
    assert!(!body.next().await.unwrap().unwrap().is_empty());
    drop(body);
    assert_eq!(
        fixture.sink.records.lock().len(),
        3,
        "neither a partial download nor body drop proves delivery"
    );
    fixture.shutdown().await;
}

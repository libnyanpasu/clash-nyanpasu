//! Delivery facts follow the controlled writer and receiver lifetimes.

use std::sync::{Arc, atomic::Ordering};

use nyanpasu_rpc_protocol::{
    RpcOwner,
    frame::{Frame, Reply},
};
use serde_json::{Value, json};

use super::{Peer, Source, delayed_mutation, details, server};
use crate::{
    audit::{
        AuditPhase, CompletionSummary, DeliveryOutcome, RpcAudit, RpcAuditOutcome, RpcAuditRecord,
        RpcAuditSink, RpcTransport, SubscriptionOutcome,
    },
    local::{
        server::{ConnectionLimits, LocalServer},
        subscriptions::DeliveryAudit,
    },
    test_support::{AuditSink, Fixture},
};

#[tokio::test]
async fn local_write_completion_is_separate_from_handler_success() {
    for (delivery, fail_handler) in [
        (DeliveryOutcome::WriteCompleted, false),
        (DeliveryOutcome::WriteCompleted, true),
        (DeliveryOutcome::WriteFailed, false),
        (DeliveryOutcome::Interrupted, false),
    ] {
        let mut fixture = Fixture::new(false).await;
        fixture.rpc.enable_local_test_command("audit_echo", None);
        let mut peer = Peer::start(
            LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
            &[],
        )
        .await;
        peer.writer.blocked.store(true, Ordering::SeqCst);
        peer.request(
            "audit_echo",
            json!({"value": "response", "fail": fail_handler}),
        )
        .await;
        peer.writer.entered.notified().await;
        {
            let records = fixture.sink.records.lock();
            assert_eq!(records.len(), 2, "no delivery fact while write is blocked");
            assert!(
                matches!(&records[1].phase, AuditPhase::Finished { outcome, .. }
                if matches!((outcome, fail_handler), (RpcAuditOutcome::Success, false) | (RpcAuditOutcome::Failure { .. }, true)))
            );
        }
        match delivery {
            DeliveryOutcome::WriteCompleted => {
                peer.resume_writer();
                assert!(matches!(peer.read().await, Frame::Response { .. }));
            }
            DeliveryOutcome::WriteFailed => {
                peer.writer.fail.store(true, Ordering::SeqCst);
                peer.resume_writer();
                peer.stop.cancelled().await;
            }
            DeliveryOutcome::Interrupted => peer.stop.cancel(),
            _ => unreachable!(),
        }
        peer.finish().await;
        {
            let records = fixture.sink.records.lock();
            assert_eq!(records.len(), 3);
            assert_eq!(records[2].phase, AuditPhase::Delivery { outcome: delivery });
            assert!(
                records
                    .iter()
                    .all(|record| record.header.invocation_id == records[0].header.invocation_id)
            );
        }
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn local_disconnect_drains_accepted_mutations() {
    let mut fixture = Fixture::new(false).await;
    fixture
        .rpc
        .enable_local_test_command("audit_echo", Some(delayed_mutation));
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    peer.request("audit_echo", Value::Null).await;
    assert_eq!(events.recv().await.unwrap().0, "mutation_entered");
    peer.disconnect().await;
    assert!(!peer.task.as_ref().unwrap().is_finished());
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    peer.finish().await;
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[1].phase,
            AuditPhase::Finished {
                outcome: RpcAuditOutcome::Success,
                completion: CompletionSummary::NotReported,
            }
        );
        assert_eq!(
            records[2].phase,
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::NotAttempted
            }
        );
    }
    fixture.shutdown().await;
}

struct TeardownSink {
    source: Arc<Source>,
    sink: Arc<AuditSink>,
}

impl RpcAuditSink for TeardownSink {
    fn record(&self, record: &RpcAuditRecord) {
        if matches!(
            record.phase,
            AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Unsubscribed
                    | SubscriptionOutcome::Disconnected
                    | SubscriptionOutcome::Failed
            }
        ) {
            assert_eq!(
                self.source.details.receiver_count(),
                0,
                "audit must follow receiver teardown"
            );
        }
        self.sink.record(record);
    }
}

#[tokio::test]
async fn unsubscribe_audit_follows_receiver_teardown() {
    let mut fixture = Fixture::new(false).await;
    let source = Source::new();
    Arc::get_mut(&mut fixture.rpc.dependencies).unwrap().audit =
        RpcAudit::new(Arc::new(TeardownSink {
            source: source.clone(),
            sink: fixture.sink.clone(),
        }));
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &["connection-details"],
    )
    .await;
    peer.subscribe("connection-details", None).await;
    let Frame::Subscribed { subscription, .. } = peer.read().await else {
        panic!("subscription established");
    };
    for sequence in 1..=2 {
        source.details.send_replace(details(sequence));
        assert!(matches!(peer.read().await, Frame::Data { .. }));
    }
    assert_eq!(
        fixture.sink.records.lock().len(),
        4,
        "only establishment and acknowledgement are audited, never data frames"
    );
    let id = peer.id();
    peer.send(Frame::Unsubscribe { id, subscription }).await;
    assert!(matches!(peer.read().await, Frame::Unsubscribed { .. }));
    assert_eq!(source.details.receiver_count(), 0);
    peer.subscribe("connection-details", None).await;
    assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
    peer.finish().await;
    {
        let records = fixture.sink.records.lock();
        let outcomes: Vec<_> = records
            .iter()
            .filter_map(|record| match record.phase {
                AuditPhase::Subscription { outcome } => Some((record.header.method, outcome)),
                _ => None,
            })
            .collect();
        assert_eq!(
            outcomes,
            [
                ("local_subscribe", SubscriptionOutcome::Established),
                ("local_subscribe", SubscriptionOutcome::Unsubscribed),
                ("local_unsubscribe", SubscriptionOutcome::Unsubscribed),
                ("local_subscribe", SubscriptionOutcome::Established),
                ("local_subscribe", SubscriptionOutcome::Disconnected),
            ]
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn shared_delivery_observation_keeps_invocation_records_separate() {
    let fixture = Fixture::new(false).await;
    let owner = RpcOwner::desktop("shared-delivery");
    let invocations: Vec<_> = (0..2)
        .map(|_| {
            fixture
                .rpc
                .audit_invocation(RpcTransport::Local, "get_profiles", &owner, None)
        })
        .collect();
    for audit in &invocations {
        crate::registry::dispatch_with_audit(audit, || {
            Ok(crate::registry::unreported(async { Ok(()) }))
        })
        .await
        .unwrap();
    }
    // One observed transport fact can be attached to several invocation chains.
    let mut delivery = crate::local::subscriptions::Delivery {
        frame: Frame::Response {
            id: nyanpasu_rpc_protocol::frame::RequestId::new(1).unwrap(),
            reply: Reply::Result(Value::Null),
        },
        written: None,
        audits: invocations.into_iter().map(DeliveryAudit::new).collect(),
    };
    for audit in &mut delivery.audits {
        audit.observe(DeliveryOutcome::WriteCompleted);
    }
    drop(delivery);
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 6);
        assert_ne!(
            records[4].header.invocation_id,
            records[5].header.invocation_id
        );
        for delivery in &records[4..] {
            assert_eq!(
                delivery.phase,
                AuditPhase::Delivery {
                    outcome: DeliveryOutcome::WriteCompleted
                }
            );
            assert_eq!(
                records
                    .iter()
                    .filter(|record| record.header.invocation_id == delivery.header.invocation_id)
                    .count(),
                3
            );
        }
    }
    fixture.shutdown().await;
}

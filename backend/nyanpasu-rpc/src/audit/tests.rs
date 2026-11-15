use std::time::{Duration, UNIX_EPOCH};

use nyanpasu_observers::audit::AuditDropped;
use nyanpasu_rpc_protocol::{RpcOwner, frame::RequestId};
use tokio::sync::mpsc;

use super::{
    AuditPhase, CompletionSummary, DeliveryOutcome, ObservationEnd, RecordingRpcAuditSink,
    RpcAudit, RpcAuditOutcome, RpcAuditRecord, RpcAuditSink as _, RpcTransport,
    SubscriptionOutcome, audit_row,
};

fn phases() -> Vec<(AuditPhase, &'static str, Option<&'static str>, bool)> {
    vec![
        (
            AuditPhase::Rejected {
                kind: "invalid_params",
                code: Some("invalid".into()),
            },
            "rejected",
            None,
            true,
        ),
        (AuditPhase::Admitted, "admitted", None, false),
        (
            finished(RpcAuditOutcome::Success),
            "finished",
            Some("success"),
            true,
        ),
        (
            finished(RpcAuditOutcome::Failure {
                kind: "application_error",
                code: Some("failed".into()),
            }),
            "finished",
            Some("failure"),
            true,
        ),
        (
            finished(RpcAuditOutcome::NotObserved),
            "finished",
            Some("not_observed"),
            true,
        ),
        (
            finished(RpcAuditOutcome::Panicked),
            "finished",
            Some("panicked"),
            true,
        ),
        (
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::WriteCompleted,
            },
            "delivery",
            Some("write_completed"),
            false,
        ),
        (
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::WriteFailed,
            },
            "delivery",
            Some("write_failed"),
            false,
        ),
        (
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::NotAttempted,
            },
            "delivery",
            Some("not_attempted"),
            false,
        ),
        (
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::Interrupted,
            },
            "delivery",
            Some("interrupted"),
            false,
        ),
        (
            AuditPhase::Delivery {
                outcome: DeliveryOutcome::ResponsePrepared,
            },
            "delivery",
            Some("response_prepared"),
            false,
        ),
        (
            AuditPhase::ObservationEnded {
                reason: ObservationEnd::FutureDropped,
            },
            "observation_ended",
            Some("outcome_unobserved"),
            false,
        ),
        (
            AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Established,
            },
            "subscription",
            Some("established"),
            true,
        ),
        (
            AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Unsubscribed,
            },
            "subscription",
            Some("unsubscribed"),
            true,
        ),
        (
            AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Disconnected,
            },
            "subscription",
            Some("disconnected"),
            true,
        ),
        (
            AuditPhase::Subscription {
                outcome: SubscriptionOutcome::Failed,
            },
            "subscription",
            Some("failed"),
            true,
        ),
    ]
}

fn finished(outcome: RpcAuditOutcome) -> AuditPhase {
    AuditPhase::Finished {
        outcome,
        completion: CompletionSummary::NotReported,
    }
}

fn record(transport: RpcTransport, phase: AuditPhase) -> RpcAuditRecord {
    let audit = RpcAudit::new(std::sync::Arc::new(super::TracingRpcAuditSink));
    let invocation = audit.invocation(
        transport,
        &RpcOwner::new("secret-cookie".into()),
        "get_profiles",
        RequestId::new(u64::MAX),
    );
    RpcAuditRecord {
        header: invocation.header,
        duration: Duration::from_nanos(123),
        phase,
    }
}

#[test]
fn only_remote_terminal_phases_reach_the_channel() {
    for transport in [
        RpcTransport::Http,
        RpcTransport::Local,
        RpcTransport::TauriIpc,
        RpcTransport::TauriProtocol,
    ] {
        for (phase, expected_phase, expected_outcome, terminal) in phases() {
            let (sender, mut receiver) = mpsc::channel(1);
            let sink = RecordingRpcAuditSink::new(sender, AuditDropped::default());
            let record = record(transport, phase);
            sink.record(&record);
            let row = receiver.try_recv().ok();
            let persisted =
                terminal && matches!(transport, RpcTransport::Http | RpcTransport::Local);
            assert_eq!(row.is_some(), persisted, "{transport:?} {expected_phase}");
            let at = UNIX_EPOCH + Duration::from_millis(456);
            let converted = audit_row(&record, at);
            assert_eq!(converted.is_some(), persisted);
            if let Some(row) = converted {
                assert_eq!(row.at, 456);
                assert_eq!(
                    row.transport,
                    if transport == RpcTransport::Http {
                        "http"
                    } else {
                        "local"
                    }
                );
                assert_eq!(row.phase, expected_phase);
                assert_eq!(row.outcome.as_deref(), expected_outcome);
                assert_eq!(row.method, "get_profiles");
                assert_eq!(row.invocation_id, record.header.invocation_id);
                assert_eq!(row.request_id, Some(u64::MAX));
                assert_eq!(row.duration_ns, 123);
                assert_eq!(row.owner, record.header.owner.as_str());
                assert!(!row.owner.contains("secret-cookie"));
            }
        }
    }
}

#[test]
fn full_counts_drops_and_closed_does_not() {
    let (sender, mut receiver) = mpsc::channel(1);
    let dropped = AuditDropped::default();
    let sink = RecordingRpcAuditSink::new(sender, dropped.clone());
    let record = record(RpcTransport::Http, finished(RpcAuditOutcome::Success));
    sink.record(&record);
    sink.record(&record);
    sink.record(&record);
    assert_eq!(dropped.count(), 2);
    assert!(receiver.try_recv().is_ok());
    assert!(receiver.try_recv().is_err());
    receiver.close();
    sink.record(&record);
    drop(receiver);
    sink.record(&record);
    assert_eq!(dropped.count(), 2);
}

#[test]
fn error_codes_are_bounded_at_utf8_boundaries() {
    for code in [
        "x".repeat(128),
        "x".repeat(129),
        format!("{}界tail", "x".repeat(127)),
        "界".repeat(50),
    ] {
        for phase in [
            AuditPhase::Rejected {
                kind: "invalid_params",
                code: Some(code.clone()),
            },
            finished(RpcAuditOutcome::Failure {
                kind: "application_error",
                code: Some(code.clone()),
            }),
        ] {
            let record = record(RpcTransport::Local, phase);
            let row = audit_row(&record, UNIX_EPOCH).unwrap();
            let saved = row.code.unwrap();
            assert!(saved.len() <= 128);
            assert!(code.starts_with(&saved));
            assert!(
                code.len() == saved.len()
                    || code[saved.len()..].chars().next().unwrap().len_utf8() + saved.len() > 128
            );
            assert!(row.kind.is_some());
        }
    }
}

#[test]
#[cfg(feature = "http")]
fn recording_sink_traces_every_transport_and_phase_at_trace() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let capture = crate::test_support::Capture::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        let (sender, _receiver) = mpsc::channel(1);
        let sink = RecordingRpcAuditSink::new(sender, AuditDropped::default());
        for transport in [
            RpcTransport::Http,
            RpcTransport::Local,
            RpcTransport::TauriIpc,
            RpcTransport::TauriProtocol,
        ] {
            for (phase, expected_phase, expected_outcome, _) in phases() {
                sink.record(&record(transport, phase));
                let events = capture.0.lock();
                let event = events.last().unwrap();
                assert_eq!(event.level, tracing::Level::TRACE);
                assert_eq!(event.fields["phase"], format!("{expected_phase:?}"));
                if let Some(outcome) = expected_outcome {
                    assert_eq!(event.fields["outcome"], format!("{outcome:?}"));
                }
            }
        }
    });
    assert_eq!(capture.0.lock().len(), 4 * phases().len());
}

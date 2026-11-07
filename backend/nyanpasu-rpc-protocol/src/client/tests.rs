use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream};

use super::{LocalClient, SubscriptionState, typed::TypedCallError};
use crate::{
    EventName,
    frame::{self, Feature, Frame, Reply, RequestId, SubscriptionErrorCode, SubscriptionTarget},
};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn handshake(stream: &mut DuplexStream) {
    assert!(matches!(
        frame::read_frame(stream).await.unwrap(),
        Some(Frame::Hello { .. })
    ));
    frame::write_frame(
        stream,
        &Frame::Accept {
            version: 1,
            features: vec![Feature::Rpc, Feature::Events, Feature::ConnectionDetails],
        },
    )
    .await
    .unwrap();
}

async fn pair(capacity: usize) -> (LocalClient, DuplexStream) {
    let (client, mut server) = tokio::io::duplex(capacity);
    let (client, ()) = tokio::join!(
        LocalClient::from_stream(client, TIMEOUT),
        handshake(&mut server)
    );
    (client.unwrap(), server)
}

async fn request(server: &mut DuplexStream) -> RequestId {
    let Some(Frame::Request { id, .. }) = frame::read_frame(server).await.unwrap() else {
        panic!("request expected")
    };
    id
}

async fn respond(server: &mut DuplexStream, id: RequestId) {
    frame::write_frame(
        server,
        &Frame::Response {
            id,
            reply: Reply::Result(json!("done")),
        },
    )
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn timed_out_local_connection_cannot_be_reused() {
    let (client, mut server) = pair(1024).await;
    let (error, _) = tokio::join!(
        client.call_typed_with_timeout::<Value>("set_storage_item", Value::Null, TIMEOUT),
        request(&mut server)
    );
    assert!(error.unwrap_err().is_outcome_unknown());
    assert!(
        !client
            .call_with_timeout("get_profiles", Value::Null, TIMEOUT)
            .await
            .unwrap_err()
            .is_outcome_unknown()
    );
    assert!(frame::read_frame(&mut server).await.unwrap().is_none());
    client.close().await;
}

#[tokio::test(start_paused = true)]
async fn partial_reply_timeout_never_retries_a_mutation() {
    let (client, mut server) = pair(1024).await;
    let mut executions = 0;
    let (error, ()) = tokio::join!(
        client.call_with_timeout("set_storage_item", Value::Null, TIMEOUT),
        async {
            let id = request(&mut server).await;
            executions += 1;
            let bytes = frame::encode(&Frame::Response {
                id,
                reply: Reply::Result(Value::Null),
            })
            .unwrap();
            server.write_all(&bytes[..7]).await.unwrap();
        }
    );
    assert!(error.unwrap_err().is_outcome_unknown());
    assert!(
        client
            .call_with_timeout("set_storage_item", Value::Null, TIMEOUT)
            .await
            .is_err()
    );
    assert!(
        frame::read_frame(&mut server).await.unwrap().is_none(),
        "no replay after a partial reply"
    );
    assert_eq!(executions, 1);
    client.close().await;
}

#[tokio::test]
async fn cancel_during_partial_request_write_keeps_stream_synchronized() {
    let (client, mut server) = pair(8).await;
    let mut call = Box::pin(client.call("set_storage_item", json!({"value": "x".repeat(1024)})));
    let mut header = [0; 4];
    tokio::select! {
        result = &mut call => panic!("exchange completed before request read: {result:?}"),
        result = server.read_exact(&mut header) => { result.unwrap(); }
    }
    // Dropping the caller must leave the transport's partial write alive.
    drop(call);
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    let next = client.call("get_profiles", Value::Null);
    let (reply, ()) = tokio::join!(next, async {
        server.read_exact(&mut body).await.unwrap();
        let Frame::Request { id, method, .. } = frame::decode(&body).unwrap() else {
            panic!("complete first request")
        };
        assert_eq!(method, "set_storage_item");
        respond(&mut server, id).await;
        let Some(Frame::Request {
            id: next_id,
            method,
            ..
        }) = frame::read_frame(&mut server).await.unwrap()
        else {
            panic!("next complete request")
        };
        assert_eq!(
            method, "get_profiles",
            "cancelled mutation is never replayed"
        );
        assert!(next_id > id);
        respond(&mut server, next_id).await;
    });
    assert_eq!(reply.unwrap(), Reply::Result(json!("done")));
    client.close().await;
    assert!(frame::read_frame(&mut server).await.unwrap().is_none());
}

#[tokio::test]
async fn cancel_during_partial_response_read_keeps_stream_synchronized() {
    let (client, mut server) = pair(8).await;
    let mut call = Box::pin(client.call("set_storage_item", Value::Null));
    let mut remainder = Vec::new();
    tokio::select! {
        result = &mut call => panic!("exchange completed before partial reply: {result:?}"),
        () = async {
            let id = request(&mut server).await;
            let bytes = frame::encode(&Frame::Response { id, reply: Reply::Result(Value::Null) }).unwrap();
            // More than duplex capacity ensures the reader consumed part of this frame.
            server.write_all(&bytes[..12]).await.unwrap();
            remainder.extend_from_slice(&bytes[12..]);
        } => {}
    }
    drop(call);
    let (reply, ()) = tokio::join!(client.call("get_profiles", Value::Null), async {
        server.write_all(&remainder).await.unwrap();
        let Some(Frame::Request { id, method, .. }) = frame::read_frame(&mut server).await.unwrap()
        else {
            panic!("next complete request")
        };
        assert_eq!(
            method, "get_profiles",
            "cancelled mutation is never replayed"
        );
        respond(&mut server, id).await;
    });
    assert_eq!(reply.unwrap(), Reply::Result(json!("done")));
    client.close().await;
    assert!(frame::read_frame(&mut server).await.unwrap().is_none());
}

#[tokio::test]
async fn all_post_submission_transport_failures_report_unknown() {
    for reply in [
        b"bad json".as_slice(),
        br#"{"type":"response","id":999,"result":null}"#.as_slice(),
        b"".as_slice(),
    ] {
        let (client, mut server) = pair(1024).await;
        let (result, ()) = tokio::join!(client.call("set_storage_item", Value::Null), async {
            request(&mut server).await;
            if !reply.is_empty() {
                server
                    .write_all(&(reply.len() as u32).to_be_bytes())
                    .await
                    .unwrap();
                server.write_all(reply).await.unwrap();
            }
            server.shutdown().await.unwrap();
        });
        assert!(result.unwrap_err().is_outcome_unknown(), "reply {reply:?}");
        client.close().await;
    }
}

#[tokio::test]
async fn local_subscription_events_replace_the_whole_value() {
    let (client, mut server) = pair(1024).await;
    let (subscription, ()) = tokio::join!(client.subscribe(EventName::CoreLogsChanged), async {
        let Some(Frame::Subscribe { id, .. }) = frame::read_frame(&mut server).await.unwrap()
        else {
            panic!("subscribe")
        };
        frame::write_frame(
            &mut server,
            &Frame::Subscribed {
                id,
                subscription: "s1".into(),
            },
        )
        .await
        .unwrap();
    });
    let mut subscription = subscription.unwrap();
    frame::write_frame(
        &mut server,
        &Frame::Reset {
            subscription: "s1".into(),
            event: "core-logs-changed".into(),
            snapshot: json!({"status": 0}),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        subscription.changed().await,
        SubscriptionState::Value(json!({"status": 0}))
    );
    for payload in [json!({"status": 1, "old": true}), json!({"status": 2})] {
        frame::write_frame(
            &mut server,
            &Frame::Event {
                subscription: "s1".into(),
                name: "core-logs-changed".into(),
                payload: payload.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            subscription.changed().await,
            SubscriptionState::Value(payload),
            "ordinary events replace the previous object, including omitted keys"
        );
    }
    client.close().await;
}

#[tokio::test]
async fn local_subscription_reset_and_details_errors_are_distinct() {
    let (client, mut server) = pair(1024).await;
    let (subscription, ()) = tokio::join!(client.subscribe(EventName::CoreLogsChanged), async {
        let Some(Frame::Subscribe { id, .. }) = frame::read_frame(&mut server).await.unwrap()
        else {
            panic!("subscribe")
        };
        frame::write_frame(
            &mut server,
            &Frame::Subscribed {
                id,
                subscription: "s1".into(),
            },
        )
        .await
        .unwrap();
    });
    let mut subscription = subscription.unwrap();
    assert_eq!(subscription.current(), SubscriptionState::Pending);
    for (frame, expected) in [
        (
            Frame::Reset {
                subscription: "s1".into(),
                event: "core-logs-changed".into(),
                snapshot: json!({"status": 1, "old": true}),
            },
            SubscriptionState::Value(json!({"status": 1, "old": true})),
        ),
        (
            Frame::Lag {
                subscription: "s1".into(),
                reason: frame::LagReason::Overflow,
                resync: true,
            },
            SubscriptionState::Lagged,
        ),
        (
            Frame::Reset {
                subscription: "s1".into(),
                event: "core-logs-changed".into(),
                snapshot: json!({"status": 2}),
            },
            SubscriptionState::Value(json!({"status": 2})),
        ),
        (
            Frame::SubscriptionError {
                target: SubscriptionTarget::Subscription("s1".into()),
                code: SubscriptionErrorCode::SnapshotUnavailable,
            },
            SubscriptionState::Failed(SubscriptionErrorCode::SnapshotUnavailable),
        ),
    ] {
        frame::write_frame(&mut server, &frame).await.unwrap();
        assert_eq!(subscription.changed().await, expected);
    }
    let (details, ()) = tokio::join!(client.subscribe_details(), async {
        let Some(Frame::Subscribe {
            id,
            stream,
            event: None,
        }) = frame::read_frame(&mut server).await.unwrap()
        else {
            panic!("details subscribe")
        };
        assert_eq!(stream, "connection-details");
        frame::write_frame(
            &mut server,
            &Frame::Subscribed {
                id,
                subscription: "d1".into(),
            },
        )
        .await
        .unwrap();
    });
    let mut details = details.unwrap();
    frame::write_frame(
        &mut server,
        &Frame::Data {
            subscription: "d1".into(),
            stream: "connection-details".into(),
            payload: json!({"sequence": 7, "connections": []}),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        details.changed().await,
        SubscriptionState::Details(json!({"sequence": 7, "connections": []}))
    );
    frame::write_frame(
        &mut server,
        &Frame::SubscriptionError {
            target: SubscriptionTarget::Subscription("d1".into()),
            code: SubscriptionErrorCode::ResourceLimit,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        details.changed().await,
        SubscriptionState::Failed(SubscriptionErrorCode::ResourceLimit)
    );
    client.close().await;
}

#[tokio::test]
async fn typed_receiving_codec_decodes_core_agent_skills_without_core() {
    use crate::wire::profiles::CoreAgentSkill;

    // This HTTP-only command exercises receiving DTOs over a fake peer, not local admission.
    let (client, mut server) = pair(4096).await;
    for expected in [
        json!([]),
        json!([
            {"name": "empty", "dir": "", "installed": false},
            {"name": "Windows", "dir": "C:\\Users\\測試\\skills", "installed": true},
            {"name": "Unix", "dir": "/home/user/skills", "installed": false}
        ]),
    ] {
        let (reply, ()) = tokio::join!(
            client.call_typed("get_core_agent_skills", json!({"coreType": "mihomo"})),
            async {
                let Some(Frame::Request { id, method, params }) =
                    frame::read_frame(&mut server).await.unwrap()
                else {
                    panic!("request expected")
                };
                assert_eq!(method, "get_core_agent_skills");
                assert_eq!(params, json!({"coreType": "mihomo"}));
                frame::write_frame(
                    &mut server,
                    &Frame::Response {
                        id,
                        reply: Reply::Result(expected.clone()),
                    },
                )
                .await
                .unwrap();
            }
        );
        let skills: Vec<CoreAgentSkill> = reply.unwrap();
        assert_eq!(serde_json::to_value(skills).unwrap(), expected);
    }
    client.close().await;
}

#[tokio::test]
async fn typed_local_client_decodes_mutations_and_domain_errors() {
    use crate::wire::receive;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteProfile {
        uid: String,
    }

    let (client, mut server) = pair(8192).await;
    let invalid_params = std::collections::BTreeMap::from([(vec!["not", "a JSON key"], ())]);
    let failure = client
        .call_typed::<()>("delete_profile", invalid_params)
        .await
        .unwrap_err();
    assert!(!failure.is_outcome_unknown());
    assert!(matches!(failure, TypedCallError::EncodeRequest { .. }));
    let cause = json!({
        "kind": "build_runtime",
        "source": {"kind": "run_pipeline", "source": {
            "kind": "execute_script", "source": {
                "kind": "start_engine", "runtime": "lua", "operation": "transform"
            }
        }}
    });
    // Both mutation variants, unit/null values, receipts, optional metadata and nested causes.
    for degraded in [false, true] {
        let mut expected = json!({
            "status": "committed", "value": null, "notifications_pending": degraded,
            "commits": [{
                "operation_id": if degraded { Some("operation-1") } else { None },
                "domain": "profiles", "source_version": 7,
                "runtime": if degraded { "deferred" } else { "applied" }
            }]
        });
        if degraded {
            expected["status"] = json!("committed_degraded");
            expected["degradations"] = json!([{
                "phase": "runtime_build", "message": "Lua unavailable", "retryable": false,
                "reason": {"code": "runtime_deferred", "cause": cause}
            }]);
        }
        let (outcome, ()) = tokio::join!(
            client.call_typed::<receive::MutationOutcome<()>>(
                "delete_profile",
                DeleteProfile {
                    uid: "profile-1".into()
                }
            ),
            async {
                let Some(Frame::Request { id, method, params }) =
                    frame::read_frame(&mut server).await.unwrap()
                else {
                    panic!("request expected")
                };
                assert_eq!(method, "delete_profile");
                assert_eq!(params, json!({"uid": "profile-1"}));
                frame::write_frame(
                    &mut server,
                    &Frame::Response {
                        id,
                        reply: Reply::Result(expected.clone()),
                    },
                )
                .await
                .unwrap();
            }
        );
        assert_eq!(serde_json::to_value(outcome.unwrap()).unwrap(), expected);
    }

    for present in [false, true] {
        let domain = json!({
            "kind": {"domain": "runtime", "error": cause},
            "message": "Lua unavailable", "detail": "remote diagnostic"
        });
        let expected = json!({
            "kind": "application_error", "message": "Lua unavailable",
            "code": if present { Some("script_engine_unavailable") } else { None },
            "retryable": if present { Some(false) } else { None },
            "operation_id": if present { Some("operation-2") } else { None },
            "domain_error": if present { Some(domain.clone()) } else { None }
        });
        let (result, ()) = tokio::join!(
            // Value accepts every successful JSON shape; a remote error must still be Err.
            client.call_typed_with_timeout::<Value>("delete_profile", (), TIMEOUT),
            async {
                let id = request(&mut server).await;
                frame::write_frame(
                    &mut server,
                    &Frame::Response {
                        id,
                        reply: Reply::Error(expected.clone()),
                    },
                )
                .await
                .unwrap();
            }
        );
        let failure = result.unwrap_err();
        assert!(!failure.is_outcome_unknown());
        let TypedCallError::Remote { error } = failure else {
            panic!("remote error expected")
        };
        assert_eq!(serde_json::to_value(&error).unwrap(), expected);
        let decoded = error.decode_domain_error::<receive::IpcError>().unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            if present { domain } else { Value::Null }
        );
    }
    // A reported domain failure leaves the connection usable for a subsequent unit result.
    let (result, ()) = tokio::join!(client.call_typed::<()>("close_log_session", ()), async {
        let id = request(&mut server).await;
        frame::write_frame(
            &mut server,
            &Frame::Response {
                id,
                reply: Reply::Result(Value::Null),
            },
        )
        .await
        .unwrap();
    });
    result.unwrap();
    client.close().await;

    // An invalid receiving shape is an unknown outcome and closes the connection, without replay.
    for reply in [
        Reply::Result(json!({"status": "unknown"})),
        Reply::Error(json!({})),
    ] {
        let (client, mut server) = pair(4096).await;
        let (result, ()) = tokio::join!(
            client.call_typed::<receive::MutationOutcome<()>>("delete_profile", ()),
            async {
                let id = request(&mut server).await;
                frame::write_frame(&mut server, &Frame::Response { id, reply })
                    .await
                    .unwrap();
            }
        );
        let failure = result.unwrap_err();
        assert!(failure.is_outcome_unknown());
        assert!(matches!(failure, TypedCallError::DecodeReply { .. }));
        assert!(frame::read_frame(&mut server).await.unwrap().is_none());
        client.close().await;
    }
}

#[test]
fn typed_client_builds_without_application_dependencies() {
    use crate::wire::{profiles::NewProfileRequest, receive::MutationOutcome};

    // Compile the public request/response API using only protocol and canonical foundation values.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateProfile {
        request: NewProfileRequest,
        file_data: Option<String>,
    }
    async fn consumer(client: &LocalClient, params: CreateProfile) -> Result<(), TypedCallError> {
        let _: MutationOutcome<nyanpasu_config::profile::ProfileId> =
            client.call_typed("create_profile", params).await?;
        Ok(())
    }
    let _ = consumer;
}

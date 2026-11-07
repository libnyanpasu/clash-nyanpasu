use nyanpasu_config::profile::Profiles;
use serde_json::{Value, json};

use crate::{
    RpcError,
    frame::{
        self, Codec, Feature, Frame, FrameError, HEADER_LEN, LagReason, MAX_FRAME_LEN,
        PROTOCOL_VERSION, RejectCode, Reply, RequestId, SubscriptionErrorCode, SubscriptionTarget,
    },
    session::Session,
};

fn id(value: u64) -> RequestId {
    RequestId::new(value).unwrap()
}

fn hello() -> Frame {
    Frame::Hello {
        versions: vec![PROTOCOL_VERSION],
        required_features: vec!["rpc".into()],
        optional_features: vec!["events".into(), "connection-details".into()],
    }
}

fn session() -> Session {
    Session::new(
        vec![Feature::Rpc, Feature::Events, Feature::ConnectionDetails],
        8,
        2,
    )
}

fn ready() -> Session {
    let mut session = session();
    let accept = session.receive(&hello()).unwrap().unwrap();
    assert!(matches!(accept, Frame::Accept { version: 1, .. }));
    session.send(&accept).unwrap();
    session
}

fn request(value: u64) -> Frame {
    Frame::Request {
        id: id(value),
        method: "get_profiles".into(),
        params: Value::Null,
    }
}

fn subscribe(value: u64) -> Frame {
    Frame::Subscribe {
        id: id(value),
        stream: "events".into(),
        event: Some("core-logs-changed".into()),
    }
}

fn reset() -> Frame {
    Frame::Reset {
        subscription: "s1".into(),
        event: "core-logs-changed".into(),
        snapshot: log_status(),
    }
}

fn event() -> Frame {
    Frame::Event {
        subscription: "s1".into(),
        name: "core-logs-changed".into(),
        payload: log_status(),
    }
}

fn log_status() -> Value {
    json!({"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}})
}

#[test]
fn unsupported_subscription_error_preserves_wire_code() {
    let failure = Frame::SubscriptionError {
        target: SubscriptionTarget::Id(id(1)),
        code: SubscriptionErrorCode::Unsupported,
    };
    let wire = br#"{"type":"subscription_error","id":1,"code":"unsupported"}"#;
    assert_eq!(frame::decode(wire).unwrap(), failure);
    let encoded = frame::encode(&failure).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&encoded[HEADER_LEN..]).unwrap(),
        serde_json::from_slice::<Value>(wire).unwrap()
    );
    assert_eq!(frame::decode(&encoded[HEADER_LEN..]).unwrap(), failure);
}

#[test]
fn local_frames_preserve_json_and_big_endian_lengths() {
    let error = RpcError {
        kind: "application",
        message: "日本語".into(),
        code: Some("busy".into()),
        retryable: Some(true),
        operation_id: Some("op-7".into()),
        domain_error: Some(Box::new(crate::RpcValue(
            json!({"kind":"operational","nested":[1,null]}),
        ))),
    };
    let frames = [
        hello(),
        request(1),
        Frame::Response {
            id: id(1),
            reply: Reply::Result(json!({"valid":[],"items":[]})),
        },
        Frame::Response {
            id: id(2),
            reply: Reply::Error(serde_json::to_value(&error).unwrap()),
        },
        subscribe(3),
        Frame::Subscribed {
            id: id(3),
            subscription: "s1".into(),
        },
        reset(),
        event(),
        Frame::Lag {
            subscription: "s1".into(),
            reason: LagReason::Overflow,
            resync: true,
        },
        Frame::Unsubscribe {
            id: id(4),
            subscription: "s1".into(),
        },
        Frame::Unsubscribed {
            id: id(4),
            subscription: "s1".into(),
        },
        Frame::Reject {
            code: RejectCode::UnsupportedVersion,
        },
        Frame::Data {
            subscription: "d1".into(),
            stream: "connection-details".into(),
            payload: json!({"sequence":7,"connections":[]}),
        },
        Frame::SubscriptionError {
            target: SubscriptionTarget::Id(id(5)),
            code: SubscriptionErrorCode::FeatureNotNegotiated,
        },
        Frame::SubscriptionError {
            target: SubscriptionTarget::Subscription("d1".into()),
            code: SubscriptionErrorCode::SnapshotUnavailable,
        },
    ];
    assert_eq!(MAX_FRAME_LEN, 4 * 1024 * 1024);
    assert_eq!(PROTOCOL_VERSION, 1);
    for frame in frames {
        let bytes = frame::encode(&frame).unwrap();
        assert_eq!(
            &bytes[..HEADER_LEN],
            &u32::try_from(bytes.len() - HEADER_LEN)
                .unwrap()
                .to_be_bytes()
        );
        assert_eq!(frame::decode(&bytes[HEADER_LEN..]).unwrap(), frame);
    }
    assert_eq!(
        serde_json::to_value(request(1)).unwrap(),
        json!({"type":"request","id":1,"method":"get_profiles","params":null})
    );
    let wire = serde_json::to_value(Frame::Response {
        id: id(1),
        reply: Reply::Error(serde_json::to_value(&error).unwrap()),
    })
    .unwrap();
    assert_eq!(wire["error"], serde_json::to_value(error).unwrap());
    assert!(wire.get("result").is_none());
}

#[test]
fn local_codec_handles_fragmented_and_coalesced_frames() {
    let frames = [hello(), request(1), request(u64::MAX)];
    let bytes: Vec<_> = frames
        .iter()
        .flat_map(|frame| frame::encode(frame).unwrap())
        .collect();
    for size in [1, 2, 3, 4, 7, bytes.len()] {
        let mut codec = Codec::new();
        let mut decoded = Vec::new();
        for chunk in bytes.chunks(size) {
            let mut remaining = chunk;
            while !remaining.is_empty() {
                let (consumed, frame) = codec.feed(remaining).unwrap();
                assert!(consumed > 0);
                remaining = &remaining[consumed..];
                decoded.extend(frame);
            }
        }
        codec.finish().unwrap();
        assert_eq!(decoded, frames, "fragment size {size}");
    }
}

#[test]
fn local_handshake_rejects_incompatible_versions_before_dispatch() {
    let mut waiting = session();
    waiting.receive(&hello()).unwrap();
    assert!(
        waiting.receive(&request(1)).is_err(),
        "accept must pass the ordered writer first"
    );
    for versions in [vec![], vec![0], vec![2, u32::MAX]] {
        let mut session = session();
        let mut greeting = hello();
        let Frame::Hello {
            versions: offered, ..
        } = &mut greeting
        else {
            unreachable!()
        };
        *offered = versions;
        assert_eq!(
            session.receive(&greeting).unwrap_err().reject_code(),
            RejectCode::UnsupportedVersion
        );
        assert!(session.receive(&request(1)).is_err());
    }
    let mut session = session();
    assert_eq!(
        session.receive(&request(1)).unwrap_err().reject_code(),
        RejectCode::InvalidFrame
    );
    assert!(
        session.receive(&hello()).is_err(),
        "protocol rejection is terminal"
    );
    let mut session = ready();
    assert_eq!(
        session.receive(&hello()).unwrap_err().reject_code(),
        RejectCode::InvalidFrame
    );
}

#[test]
fn local_required_features_are_negotiated() {
    let mut server = Session::new(vec![Feature::Rpc, Feature::Events], 8, 2);
    let greeting = Frame::Hello {
        versions: vec![99, 1, 0],
        required_features: vec!["rpc".into(), "events".into()],
        optional_features: vec!["unknown".into(), "connection-details".into()],
    };
    assert_eq!(
        server.receive(&greeting).unwrap(),
        Some(Frame::Accept {
            version: 1,
            features: vec![Feature::Rpc, Feature::Events]
        })
    );
    for required in [vec![], vec!["events"], vec!["rpc", "unknown"]] {
        let mut session = session();
        assert_eq!(
            session
                .receive(&Frame::Hello {
                    versions: vec![1],
                    required_features: required.into_iter().map(str::to_owned).collect(),
                    optional_features: vec!["rpc".into()]
                })
                .unwrap_err()
                .reject_code(),
            RejectCode::UnsupportedFeature
        );
        assert!(session.receive(&request(1)).is_err());
    }
    let mut server = Session::new(vec![Feature::Events], 8, 2);
    assert_eq!(
        server.receive(&hello()).unwrap_err().reject_code(),
        RejectCode::UnsupportedFeature
    );
    let mut server = Session::new(vec![Feature::Rpc], 8, 2);
    let accept = server.receive(&hello()).unwrap().unwrap();
    server.send(&accept).unwrap();
    assert_eq!(
        server.receive(&subscribe(1)).unwrap(),
        Some(Frame::SubscriptionError {
            target: SubscriptionTarget::Id(id(1)),
            code: SubscriptionErrorCode::FeatureNotNegotiated
        })
    );
    assert!(server.receive(&request(2)).unwrap().is_none());
}

#[test]
fn local_frame_limits_and_duplicate_ids_are_enforced() {
    for len in [MAX_FRAME_LEN + 1, u32::MAX as usize] {
        let mut codec = Codec::new();
        assert!(matches!(
            codec.feed(&(len as u32).to_be_bytes()),
            Err(FrameError::ValidateLength { .. })
        ));
        assert!(matches!(
            codec.feed(&frame::encode(&request(1)).unwrap()),
            Err(FrameError::ResumeFailed)
        ));
    }
    let mut frame = request(1);
    let Frame::Request { params, .. } = &mut frame else {
        unreachable!()
    };
    *params = json!("x".repeat(MAX_FRAME_LEN));
    assert!(matches!(
        frame::encode(&frame),
        Err(FrameError::ValidateLength { .. })
    ));
    let empty = frame::encode(&request(1)).unwrap().len() - HEADER_LEN;
    let exact = Frame::Request {
        id: id(1),
        method: "get_profiles".into(),
        params: json!("x".repeat(MAX_FRAME_LEN - empty + 2)),
    };
    let bytes = frame::encode(&exact).unwrap();
    assert_eq!(bytes.len(), MAX_FRAME_LEN + HEADER_LEN);
    assert_eq!(Codec::new().feed(&bytes).unwrap().1, Some(exact));
    let mut session = ready();
    session.receive(&request(7)).unwrap();
    assert_eq!(
        session.receive(&request(7)).unwrap_err().reject_code(),
        RejectCode::InvalidRequestId
    );
    let mut session = ready();
    session.receive(&request(u64::MAX)).unwrap();
    assert_eq!(
        session.receive(&request(1)).unwrap_err().reject_code(),
        RejectCode::InvalidRequestId
    );
    assert!(RequestId::new(0).is_none());
}

#[test]
fn completed_request_ids_cannot_be_reused() {
    for reply in [
        Reply::Result(Value::Null),
        Reply::Error(serde_json::to_value(RpcError::unknown_method("missing")).unwrap()),
    ] {
        for retried in [2, 1] {
            let mut session = ready();
            for value in [1, 2] {
                session.receive(&request(value)).unwrap();
                session
                    .send(&Frame::Response {
                        id: id(value),
                        reply: reply.clone(),
                    })
                    .unwrap();
            }
            assert_eq!(
                session
                    .receive(&request(retried))
                    .unwrap_err()
                    .reject_code(),
                RejectCode::InvalidRequestId,
                "completed ID {retried} cannot be retried with no pending requests"
            );
        }
    }
    let mut session = ready();
    session.receive(&request(1)).unwrap();
    assert!(
        session
            .send(&Frame::Response {
                id: id(2),
                reply: Reply::Result(Value::Null)
            })
            .is_err()
    );
    let mut session = ready();
    session.receive(&subscribe(1)).unwrap();
    assert!(
        session
            .send(&Frame::Response {
                id: id(1),
                reply: Reply::Result(Value::Null)
            })
            .is_err(),
        "subscription and request replies cannot be mixed"
    );
}

#[test]
fn duplicate_responses_are_rejected() {
    for reply in [
        Reply::Result(Value::Null),
        Reply::Error(json!({"kind":"unknown_method"})),
    ] {
        let mut session = ready();
        session.receive(&request(1)).unwrap();
        let response = Frame::Response { id: id(1), reply };
        session.send(&response).unwrap();
        assert_eq!(
            session.send(&response).unwrap_err().reject_code(),
            RejectCode::InvalidFrame
        );
    }
}

fn assert_unsubscribe_during_establishment(subscribe: Frame, late_frames: &[Frame]) {
    for complete_before_ack in [true, false] {
        for late in late_frames {
            let mut session = ready();
            session.receive(&subscribe).unwrap();
            session
                .receive(&Frame::Unsubscribe {
                    id: id(2),
                    subscription: "s1".into(),
                })
                .unwrap();
            if complete_before_ack {
                session.subscribed(id(1)).unwrap();
                if matches!(&subscribe, Frame::Subscribe { event: Some(_), .. }) {
                    session.send(&reset()).unwrap();
                }
            }
            session
                .send(&Frame::Unsubscribed {
                    id: id(2),
                    subscription: "s1".into(),
                })
                .unwrap();
            if !complete_before_ack {
                let Frame::Subscribed { subscription, .. } = session.subscribed(id(1)).unwrap()
                else {
                    panic!("expected establishment acknowledgement")
                };
                assert_ne!(
                    subscription, "s1",
                    "acknowledged ID cannot be activated later"
                );
            }
            assert_eq!(
                session.send(late).unwrap_err().reject_code(),
                RejectCode::InvalidFrame,
                "no {late:?} after unsubscribe ack; completion before ack: {complete_before_ack}"
            );
        }
    }
}

#[test]
fn pending_events_unsubscribe_ack_prevents_late_frames() {
    assert_unsubscribe_during_establishment(
        subscribe(1),
        &[
            event(),
            reset(),
            Frame::Lag {
                subscription: "s1".into(),
                reason: LagReason::Overflow,
                resync: true,
            },
            Frame::SubscriptionError {
                target: SubscriptionTarget::Subscription("s1".into()),
                code: SubscriptionErrorCode::SnapshotUnavailable,
            },
        ],
    );
}

#[test]
fn pending_details_unsubscribe_ack_prevents_late_frames() {
    assert_unsubscribe_during_establishment(
        Frame::Subscribe {
            id: id(1),
            stream: "connection-details".into(),
            event: None,
        },
        &[
            Frame::Data {
                subscription: "s1".into(),
                stream: "connection-details".into(),
                payload: json!({"sequence":7,"connections":[]}),
            },
            Frame::SubscriptionError {
                target: SubscriptionTarget::Subscription("s1".into()),
                code: SubscriptionErrorCode::SnapshotUnavailable,
            },
        ],
    );
}

fn subscription_output_cases() -> [(Frame, Vec<Frame>); 2] {
    let failure = Frame::SubscriptionError {
        target: SubscriptionTarget::Subscription("s1".into()),
        code: SubscriptionErrorCode::SnapshotUnavailable,
    };
    [
        (
            subscribe(1),
            vec![
                event(),
                reset(),
                Frame::Lag {
                    subscription: "s1".into(),
                    reason: LagReason::Overflow,
                    resync: true,
                },
                failure.clone(),
            ],
        ),
        (
            Frame::Subscribe {
                id: id(1),
                stream: "connection-details".into(),
                event: None,
            },
            vec![
                Frame::Data {
                    subscription: "s1".into(),
                    stream: "connection-details".into(),
                    payload: json!({"sequence":7,"connections":[]}),
                },
                failure,
            ],
        ),
    ]
}

#[test]
fn failed_establishment_keeps_unsubscribed_ids_retired() {
    for fail_before_ack in [true, false] {
        for (subscription, late_frames) in subscription_output_cases() {
            for late in late_frames {
                let mut session = ready();
                assert_eq!(session.receive(&subscription).unwrap(), None);
                assert_eq!(
                    session
                        .receive(&Frame::Unsubscribe {
                            id: id(2),
                            subscription: "s1".into(),
                        })
                        .unwrap(),
                    None
                );
                let failure = Frame::SubscriptionError {
                    target: SubscriptionTarget::Id(id(1)),
                    code: SubscriptionErrorCode::SnapshotUnavailable,
                };
                assert_eq!(
                    serde_json::to_value(&failure).unwrap(),
                    json!({"type":"subscription_error","id":1,"code":"snapshot_unavailable"}),
                    "failed establishment correlates the request, without a subscription ack"
                );
                if fail_before_ack {
                    session.send(&failure).unwrap();
                }
                session
                    .send(&Frame::Unsubscribed {
                        id: id(2),
                        subscription: "s1".into(),
                    })
                    .unwrap();
                if !fail_before_ack {
                    session.send(&failure).unwrap();
                }
                assert_eq!(session.receive(&subscribe(3)).unwrap(), None);
                assert_eq!(
                    session.subscribed(id(3)).unwrap(),
                    Frame::Subscribed {
                        id: id(3),
                        subscription: "s2".into(),
                    },
                    "s1 stays retired even when establishment fails before ack: {fail_before_ack}"
                );
                assert_eq!(
                    session.send(&late).unwrap_err().reject_code(),
                    RejectCode::InvalidFrame,
                    "no {late:?} for failed/retired subscription; failure before ack: {fail_before_ack}"
                );
            }
        }
    }
}

#[test]
fn simultaneous_unsubscribes_are_idempotent_and_stop_delivery_at_first_ack() {
    // A rejected output closes the session; use a fresh one for each frame/boundary.
    for check_after_first_ack in [true, false] {
        for (subscription, late_frames) in subscription_output_cases() {
            for late in late_frames {
                let mut session = ready();
                session.receive(&subscription).unwrap();
                session.subscribed(id(1)).unwrap();
                if matches!(&subscription, Frame::Subscribe { event: Some(_), .. }) {
                    session.send(&reset()).unwrap();
                    session.send(&event()).unwrap();
                } else {
                    session
                        .send(&Frame::Data {
                            subscription: "s1".into(),
                            stream: "connection-details".into(),
                            payload: json!({"sequence":7,"connections":[]}),
                        })
                        .unwrap();
                }
                for value in [2, 3] {
                    assert_eq!(
                        session
                            .receive(&Frame::Unsubscribe {
                                id: id(value),
                                subscription: "s1".into(),
                            })
                            .unwrap(),
                        None
                    );
                }
                session
                    .send(&Frame::Unsubscribed {
                        id: id(2),
                        subscription: "s1".into(),
                    })
                    .unwrap();
                if !check_after_first_ack {
                    session
                        .send(&Frame::Unsubscribed {
                            id: id(3),
                            subscription: "s1".into(),
                        })
                        .unwrap();
                    assert_eq!(session.receive(&request(4)).unwrap(), None);
                    session
                        .send(&Frame::Response {
                            id: id(4),
                            reply: Reply::Result(Value::Null),
                        })
                        .unwrap();
                }
                assert_eq!(
                    session.send(&late).unwrap_err().reject_code(),
                    RejectCode::InvalidFrame,
                    "no {late:?} after the first ack, including while the second is pending"
                );
            }
        }
    }
}

#[test]
fn subscription_ids_skip_acknowledged_unsubscribes() {
    for (retired, next) in [
        ("s1", Some("s2")),
        ("s9", Some("s10")),
        ("s01", Some("s1")),
        ("s+1", Some("s1")),
        ("unknown", Some("s1")),
        ("s18446744073709551615", None),
    ] {
        let mut session = ready();
        session.receive(&subscribe(1)).unwrap();
        for value in 2..=10 {
            session
                .receive(&Frame::Unsubscribe {
                    id: id(value),
                    subscription: retired.into(),
                })
                .unwrap();
            session
                .send(&Frame::Unsubscribed {
                    id: id(value),
                    subscription: retired.into(),
                })
                .unwrap();
        }
        let Some(next) = next else {
            assert_eq!(
                session.subscribed(id(1)).unwrap_err().reject_code(),
                RejectCode::InvalidFrame,
                "exhausted subscription IDs must not wrap"
            );
            continue;
        };
        assert_eq!(
            session.subscribed(id(1)).unwrap(),
            Frame::Subscribed {
                id: id(1),
                subscription: next.into(),
            },
            "retired {retired}"
        );
        session
            .send(&Frame::Reset {
                subscription: next.into(),
                event: "core-logs-changed".into(),
                snapshot: log_status(),
            })
            .unwrap();
        session
            .send(&Frame::Event {
                subscription: next.into(),
                name: "core-logs-changed".into(),
                payload: log_status(),
            })
            .unwrap();
    }
}

#[test]
fn unsubscribe_ack_prevents_late_events() {
    for details in [false, true] {
        let mut session = ready();
        let subscription = if details {
            Frame::Subscribe {
                id: id(1),
                stream: "connection-details".into(),
                event: None,
            }
        } else {
            subscribe(1)
        };
        session.receive(&subscription).unwrap();
        session.subscribed(id(1)).unwrap();
        if !details {
            session.send(&reset()).unwrap();
        }
        let data = if details {
            Frame::Data {
                subscription: "s1".into(),
                stream: "connection-details".into(),
                payload: json!({"sequence":7,"connections":[]}),
            }
        } else {
            event()
        };
        session.send(&data).unwrap();
        for value in [2, 3] {
            session
                .receive(&Frame::Unsubscribe {
                    id: id(value),
                    subscription: "s1".into(),
                })
                .unwrap();
            session
                .send(&Frame::Unsubscribed {
                    id: id(value),
                    subscription: "s1".into(),
                })
                .unwrap();
        }
        assert!(
            session.send(&data).is_err(),
            "no delivery after unsubscribe acknowledgement"
        );
    }
    let mut session = ready();
    session
        .receive(&Frame::Unsubscribe {
            id: id(1),
            subscription: "unknown".into(),
        })
        .unwrap();
    session
        .send(&Frame::Unsubscribed {
            id: id(1),
            subscription: "unknown".into(),
        })
        .unwrap();
}

#[test]
fn subscription_ack_and_lag_order_are_preserved() {
    let mut session = ready();
    session.receive(&subscribe(1)).unwrap();
    assert!(session.send(&event()).is_err(), "no data before ack");
    let mut session = ready();
    session.receive(&subscribe(1)).unwrap();
    session.subscribed(id(1)).unwrap();
    assert!(
        session.send(&event()).is_err(),
        "initial reset is mandatory"
    );
    for reset_after_lag in [true, false] {
        let mut session = ready();
        session.receive(&subscribe(1)).unwrap();
        assert_eq!(
            session.subscribed(id(1)).unwrap(),
            Frame::Subscribed {
                id: id(1),
                subscription: "s1".into()
            }
        );
        session.send(&reset()).unwrap();
        session.send(&event()).unwrap();
        session
            .send(&Frame::Lag {
                subscription: "s1".into(),
                reason: LagReason::Overflow,
                resync: true,
            })
            .unwrap();
        if reset_after_lag {
            session.send(&reset()).unwrap();
        }
        assert_eq!(session.send(&event()).is_ok(), reset_after_lag);
    }
}

#[test]
fn malformed_frames_never_dispatch() {
    for body in [
        "",
        r#"{"#,
        r#"{"type":"request","id":1,"method":"x","params":null}x"#,
        r#"{"type":"unknown"}"#,
        r#"{"type":"request","id":0,"method":"x","params":null}"#,
        r#"{"type":"request","id":-1,"method":"x","params":null}"#,
        r#"{"type":"request","id":1.0,"method":"x","params":null}"#,
        r#"{"type":"request","id":18446744073709551616,"method":"x","params":null}"#,
        r#"{"type":"request","id":1,"method":7,"params":null}"#,
        r#"{"type":"hello","versions":[4294967296],"requiredFeatures":["rpc"],"optionalFeatures":[]}"#,
        r#"{"type":"response","id":1,"result":null,"error":{}}"#,
        r#"{"type":"response","id":1}"#,
        r#"{"type":"subscription_error","id":1,"subscription":"s1","code":"resource_limit"}"#,
    ] {
        let mut codec = Codec::new();
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(body.as_bytes());
        let mut server = ready();
        let mut dispatched = 0;
        match codec.feed(&bytes) {
            Err(FrameError::Decode { .. }) => server.close(),
            Ok((_, Some(frame))) if server.receive(&frame).is_ok() => dispatched += 1,
            result => panic!("expected malformed frame rejection for {body}: {result:?}"),
        }
        assert_eq!(dispatched, 0, "{body}");
        assert!(server.receive(&request(9)).is_err());
    }
    let bytes = frame::encode(&request(1)).unwrap();
    for cut in 1..bytes.len() {
        let mut codec = Codec::new();
        assert!(codec.feed(&bytes[..cut]).unwrap().1.is_none());
        assert!(matches!(codec.finish(), Err(FrameError::ReadTruncated)));
    }
    assert!(matches!(
        frame::decode(&[0xff]),
        Err(FrameError::Decode { .. })
    ));
}

#[test]
fn canonical_local_examples_decode_as_real_command_results() {
    // These are the version-1 wire examples, independent of Rust field spelling.
    let examples = r#"
{"type":"hello","versions":[1],"requiredFeatures":["rpc"],"optionalFeatures":["events","connection-details"]}
{"type":"accept","version":1,"features":["rpc","events","connection-details"]}
{"type":"request","id":1,"method":"get_profiles","params":null}
{"type":"response","id":1,"result":{"valid":[],"items":[]}}
{"type":"subscribe","id":2,"stream":"events","event":"core-logs-changed"}
{"type":"subscribed","id":2,"subscription":"s1"}
{"type":"reset","subscription":"s1","event":"core-logs-changed","snapshot":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"event","subscription":"s1","name":"core-logs-changed","payload":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"lag","subscription":"s1","reason":"overflow","resync":true}
{"type":"reset","subscription":"s1","event":"core-logs-changed","snapshot":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"unsubscribe","id":3,"subscription":"s1"}
{"type":"unsubscribed","id":3,"subscription":"s1"}
{"type":"reject","code":"unsupported_version"}
{"type":"subscribe","id":4,"stream":"connection-details"}
{"type":"subscribed","id":4,"subscription":"d1"}
{"type":"data","subscription":"d1","stream":"connection-details","payload":{"sequence":7,"connections":[]}}
{"type":"subscription_error","id":5,"code":"feature_not_negotiated"}
{"type":"subscription_error","id":6,"code":"unknown_stream"}
{"type":"subscription_error","id":7,"code":"resource_limit"}
{"type":"subscription_error","subscription":"d1","code":"snapshot_unavailable"}
"#;
    for example in examples.trim().lines() {
        let decoded = frame::decode(example.as_bytes()).unwrap();
        let encoded = frame::encode(&decoded).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&encoded[HEADER_LEN..]).unwrap(),
            serde_json::from_str::<Value>(example).unwrap()
        );
    }
    let response =
        frame::decode(br#"{"type":"response","id":1,"result":{"valid":[],"items":[]}}"#).unwrap();
    let Frame::Response {
        id: request_id,
        reply: Reply::Result(result),
    } = response
    else {
        panic!("expected result")
    };
    assert_eq!(request_id, id(1));
    let profiles: Profiles = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(profiles).unwrap(),
        result,
        "optional document fields stay omitted"
    );
    for invalid in [
        json!(false),
        json!({"valid":false,"items":[]}),
        json!({"valid":[],"items":false}),
    ] {
        assert!(serde_json::from_value::<Profiles>(invalid).is_err());
    }
    assert_eq!(
        serde_json::to_value(hello()).unwrap(),
        json!({"type":"hello","versions":[1],"requiredFeatures":["rpc"],"optionalFeatures":["events","connection-details"]})
    );
    assert_eq!(
        serde_json::to_value(reset()).unwrap(),
        json!({"type":"reset","subscription":"s1","event":"core-logs-changed","snapshot":log_status()})
    );
}

#[test]
fn local_session_bounds_streams_and_correlates_control_errors() {
    for event in crate::EventName::ALL {
        let mut session = ready();
        let result = session
            .receive(&Frame::Subscribe {
                id: id(1),
                stream: "events".into(),
                event: Some(event.as_str().into()),
            })
            .unwrap();
        if matches!(
            event.as_str(),
            "configuration-status-changed"
                | "core-status-changed-event"
                | "service-status-changed-event"
                | "app-update-state-changed"
                | "core-logs-changed"
        ) {
            assert!(result.is_none());
        } else {
            assert_eq!(
                result,
                Some(Frame::SubscriptionError {
                    target: SubscriptionTarget::Id(id(1)),
                    code: SubscriptionErrorCode::UnknownStream
                })
            );
        }
    }
    let mut session = ready();
    session.receive(&subscribe(1)).unwrap();
    session.receive(&subscribe(2)).unwrap();
    assert_eq!(
        session.receive(&subscribe(3)).unwrap(),
        Some(Frame::SubscriptionError {
            target: SubscriptionTarget::Id(id(3)),
            code: SubscriptionErrorCode::ResourceLimit
        })
    );
    session
        .send(&Frame::SubscriptionError {
            target: SubscriptionTarget::Id(id(1)),
            code: SubscriptionErrorCode::SnapshotUnavailable,
        })
        .unwrap();
    session.receive(&subscribe(4)).unwrap();
    assert_eq!(
        session.subscribed(id(2)).unwrap(),
        Frame::Subscribed {
            id: id(2),
            subscription: "s1".into()
        }
    );
    session
        .send(&Frame::SubscriptionError {
            target: SubscriptionTarget::Subscription("s1".into()),
            code: SubscriptionErrorCode::ResourceLimit,
        })
        .unwrap();
    assert_eq!(
        session.subscribed(id(4)).unwrap(),
        Frame::Subscribed {
            id: id(4),
            subscription: "s2".into()
        }
    );
}

#[cfg(feature = "frame-io")]
#[tokio::test]
async fn local_frame_io_preserves_boundaries_and_rejects_truncation() {
    let mut bytes = Vec::new();
    frame::write_frame(&mut bytes, &hello()).await.unwrap();
    frame::write_frame(&mut bytes, &request(1)).await.unwrap();
    let mut input = bytes.as_slice();
    assert_eq!(frame::read_frame(&mut input).await.unwrap(), Some(hello()));
    assert_eq!(
        frame::read_frame(&mut input).await.unwrap(),
        Some(request(1))
    );
    assert_eq!(frame::read_frame(&mut input).await.unwrap(), None);
    for cut in 1..frame::encode(&hello()).unwrap().len() {
        assert!(matches!(
            frame::read_frame(&mut &bytes[..cut]).await,
            Err(FrameError::ReadTruncated)
        ));
    }
    let header = ((MAX_FRAME_LEN + 1) as u32).to_be_bytes();
    assert!(matches!(
        frame::read_frame(&mut header.as_slice()).await,
        Err(FrameError::ValidateLength { .. })
    ));
}

#[cfg(feature = "frame-io")]
#[tokio::test]
async fn local_frame_io_errors_preserve_step_and_source() {
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    struct FailingIo {
        fail_write: bool,
    }

    impl AsyncRead for FailingIo {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionReset)))
        }
    }

    impl AsyncWrite for FailingIo {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(if self.fail_write {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            } else {
                Ok(bytes.len())
            })
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionAborted)))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    let error = frame::read_frame(&mut FailingIo { fail_write: false })
        .await
        .unwrap_err();
    assert!(
        matches!(error, FrameError::ReadFrame { source } if source.kind() == io::ErrorKind::ConnectionReset)
    );
    let error = frame::write_frame(&mut FailingIo { fail_write: true }, &hello())
        .await
        .unwrap_err();
    assert!(
        matches!(error, FrameError::WriteFrame { source } if source.kind() == io::ErrorKind::BrokenPipe)
    );
    let error = frame::write_frame(&mut FailingIo { fail_write: false }, &hello())
        .await
        .unwrap_err();
    assert!(
        matches!(error, FrameError::FlushFrame { source } if source.kind() == io::ErrorKind::ConnectionAborted)
    );
}

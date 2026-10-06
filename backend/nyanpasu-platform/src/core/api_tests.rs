use clash_api::ProxyName;
use nyanpasu_application::core::api::*;
use nyanpasu_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
use std::{sync::Arc, time::Duration};

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, body::Body, response::Response, routing::get};
    use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
    use nyanpasu_ipc::api::{
        core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
        status::{CoreControllerInfo, CoreStateDetail},
    };
    use tokio::sync::{Notify, watch};

    use super::*;
    use nyanpasu_application::core::{
        CoreClient,
        endpoint::{
            ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
        },
    };

    #[test]
    fn service_wire_submission_preserves_alpha_core_type() {
        use camino::Utf8PathBuf;
        use nyanpasu_core_manager::{
            ConfigInput, CoreCommand, CoreCommandEnvelope, CoreKind, CoreSpec, InstanceOptions,
            OperationId, ReconcileRequest,
        };
        use nyanpasu_utils::core::{ClashCoreType, CoreType};

        let core_type = CoreType::Clash(ClashCoreType::MihomoAlpha);
        let request = super::super::endpoint::wire_submit_request(&CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: CoreSpec {
                        kind: CoreKind::Mihomo,
                        binary_path: Utf8PathBuf::from("mihomo-alpha"),
                        version: None,
                        features: vec![],
                    },
                    config: ConfigInput::Inline {
                        bytes: b"proxies: []".to_vec(),
                        expected_digest: None,
                    },
                    options: InstanceOptions::default(),
                    expected_applied: None,
                })),
            },
            core_type: Some(core_type.clone()),
        })
        .unwrap();
        let nyanpasu_ipc::api::core::v2::CoreCommandInfo::Reconcile {
            core_type: wire, ..
        } = request.command
        else {
            panic!("expected reconcile wire command");
        };
        assert_eq!(wire.into_owned(), core_type);
    }

    pub(crate) struct Endpoint {
        pub(super) host: ExecutionHost,
        pub(crate) binding: watch::Sender<Option<CoreApiConnection>>,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for Endpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
            Ok(self.binding.borrow().clone())
        }

        fn api_backend(
            &self,
            binding: &CoreApiConnection,
        ) -> Result<Arc<dyn InstanceApiPort>, ApiError> {
            crate::core::clash_api_backend(binding)
        }

        async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
            Ok(Some(Box::pin(futures::stream::unfold(
                self.binding.subscribe(),
                |mut rx| async move {
                    rx.changed().await.ok()?;
                    Some((Ok(()), rx))
                },
            ))))
        }

        async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
            Ok(CoreStatusSnapshot {
                controller: None,
                state: Some(if self.binding.borrow().is_some() {
                    CoreStateDetail::Running { epoch: 1, pid: 7 }
                } else {
                    CoreStateDetail::Stopped { reason: None }
                }),
                state_changed_at: 0,
                revision: None,
                source_hash: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }

        async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
            if !matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Stop
            ) {
                return Err(CoreError::new(
                    CoreErrorKind::Internal,
                    "test accepts only stop",
                    false,
                ));
            }
            self.binding.send_replace(None);
            Ok(OperationInfo {
                id: submission.envelope.operation_id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(OperationOutputInfo::Stopped),
                error: None,
            })
        }

        async fn wait_operation(&self, _: OperationId, _: Duration) -> Option<OperationInfo> {
            None
        }
    }

    pub(crate) fn endpoint(url: String) -> Arc<Endpoint> {
        let (binding, _) = watch::channel(Some(CoreApiConnection {
            instance_id: "first-process".into(),
            controller: CoreControllerInfo::Http(url),
            secret: None,
        }));
        Arc::new(Endpoint {
            binding,
            host: ExecutionHost::Local,
        })
    }

    pub(crate) async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (url, task)
    }

    async fn revoked(api: &ApiClient) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !api.is_revoked() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn config_reads_accept_partial_responses_and_reject_retired_instances() {
        let (url, server) = server(Router::new().route(
            "/configs",
            get(|| async { Json(serde_json::json!({"mixed-port":7890,"mode":"rule"})) }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let config = api.configs().await.unwrap();
        assert_eq!(config.mixed_port, Some(7890));
        assert_eq!(config.allow_lan, None);
        endpoint.binding.send_replace(None);
        assert!(matches!(api.configs().await, Err(ApiError::Stale)));
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn provider_reads_preserve_order_and_reject_retired_instances() {
        let (url, server) = server(Router::new().route(
            "/providers/rules/",
            get(|| async {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"providers":{"z":{"name":"z"},"a":{"name":"a"}}}"#,
                    ))
                    .unwrap()
            }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let providers = api.rule_providers().await.unwrap();
        assert_eq!(
            providers
                .keys()
                .map(|name| name.as_str())
                .collect::<Vec<_>>(),
            ["z", "a"]
        );
        endpoint.binding.send_replace(None);
        assert!(matches!(api.rule_providers().await, Err(ApiError::Stale)));
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn rules_and_provider_refresh_use_the_bound_client() {
        use axum::{extract::Path, http::StatusCode, routing::put};
        let (url, server) = server(
            Router::new()
                .route(
                    "/rules/",
                    get(|| async {
                        Json(serde_json::json!({"rules":[
                            {"type":"Match","payload":"","proxy":"DIRECT"}
                        ]}))
                    }),
                )
                .route(
                    "/providers/rules/{name}/",
                    put(|Path(name): Path<String>| async move {
                        assert_eq!(name, "rules/日本 ?#");
                        StatusCode::NO_CONTENT
                    }),
                ),
        )
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let rules = api.rules().await.unwrap();
        assert_eq!(rules[0].proxy, "DIRECT");
        assert_eq!(rules[0].index, None);
        let provider = clash_api::RuleProviderName::new("rules/日本 ?#");
        api.update_rule_provider(&provider).await.unwrap();
        endpoint.binding.send_replace(None);
        assert!(matches!(api.rules().await, Err(ApiError::Stale)));
        assert!(matches!(
            api.update_rule_provider(&provider).await,
            Err(ApiError::Stale)
        ));
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn same_epoch_and_pid_replacement_revokes_every_clone_without_reviving() {
        let endpoint = endpoint("http://127.0.0.1:1/".into());
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let first = core.api_client().await.unwrap();
        let clone = first.clone();
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "second-process".into());
        revoked(&first).await;
        assert!(matches!(clone.version().await, Err(ApiError::Stale)));
        let second = core.api_client().await.unwrap();
        assert_eq!(second.instance_id(), "second-process");
        assert!(!second.is_revoked());
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "first-process".into());
        assert!(matches!(first.version().await, Err(ApiError::Stale)));
        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn unchanged_binding_keeps_capability_and_shutdown_revokes_it() {
        let (url, server) = server(Router::new().route(
            "/version",
            get(|| async { Json(serde_json::json!({"version":"test"})) }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        // Hot patch notification: revision may change, process and credentials do not.
        endpoint.binding.send_modify(|_| {});
        assert_eq!(api.version().await.unwrap().version, "test");
        let reacquired = core.api_client().await.unwrap();
        assert!(api.same_instance(&reacquired));
        core.shutdown().await.unwrap();
        assert!(matches!(api.version().await, Err(ApiError::Stale)));
        assert!(core.api_client().await.is_err());
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn changed_secret_or_endpoint_revokes_cached_capability() {
        for change_secret in [true, false] {
            let endpoint = endpoint("http://127.0.0.1:1/".into());
            let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
            let api = core.api_client().await.unwrap();
            endpoint.binding.send_modify(|binding| {
                let binding = binding.as_mut().unwrap();
                if change_secret {
                    binding.secret = Some("replacement-secret".into());
                } else {
                    binding.controller = CoreControllerInfo::Http("http://127.0.0.1:2/".into());
                }
            });
            // Acquisition uses an authoritative binding, independent of the status pump.
            let replacement = core.api_client().await.unwrap();
            revoked(&api).await;
            assert!(!replacement.is_revoked());
            assert!(!format!("{replacement:?}").contains("replacement-secret"));
            core.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn invalidation_cancels_a_partially_received_response_body() {
        let started = Arc::new(Notify::new());
        let signal = started.clone();
        let (url, server) = server(Router::new().route(
            "/version",
            get(move || {
                let signal = signal.clone();
                async move {
                    use futures::StreamExt;
                    let first = futures::stream::once(async move {
                        signal.notify_one();
                        Ok::<_, std::io::Error>("{\"version\":")
                    });
                    Response::new(Body::from_stream(first.chain(futures::stream::pending())))
                }
            }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let call = tokio::spawn(async move { api.version().await });
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        endpoint.binding.send_replace(None);
        let result = tokio::time::timeout(Duration::from_secs(2), call)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ApiError::Stale)));
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn actor_termination_revokes_outstanding_client() {
        let endpoint = endpoint("http://127.0.0.1:1/".into());
        let core = CoreClient::spawn(endpoint).await.unwrap();
        let api = core.api_client().await.unwrap();
        core.shutdown().await.unwrap();
        revoked(&api).await;
        assert!(matches!(api.version().await, Err(ApiError::Stale)));
    }
    #[tokio::test]
    async fn handoff_revokes_even_when_target_has_the_same_binding() {
        let source = endpoint("http://127.0.0.1:1/".into());
        let (binding, _) = watch::channel(source.binding.borrow().clone());
        let target = Arc::new(Endpoint {
            binding,
            host: ExecutionHost::Service,
        });
        let core = CoreClient::spawn(source).await.unwrap();
        let old = core.api_client().await.unwrap();
        core.change_host_from(target, false).await.unwrap();
        assert!(matches!(old.version().await, Err(ApiError::Stale)));
        let new = core.api_client().await.unwrap();
        assert!(!new.is_revoked());
        core.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod stream_tests {
    use super::{
        tests::{endpoint, server},
        *,
    };
    use axum::{
        Router,
        extract::{State, WebSocketUpgrade, ws::Message},
        response::IntoResponse,
        routing::get,
    };
    use nyanpasu_application::core::CoreClient;
    use tokio::sync::mpsc;

    async fn idle(
        State(closed): State<mpsc::UnboundedSender<()>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            socket
                .send(Message::Text(r#"{"up":1,"down":2}"#.into()))
                .await
                .unwrap();
            while socket.recv().await.is_some() {}
            let _ = closed.send(());
        })
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn service_named_pipe_supports_rest_and_websocket() {
        use axum::Json;
        use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

        struct PipeListener {
            path: String,
            next: NamedPipeServer,
        }

        impl axum::serve::Listener for PipeListener {
            type Io = NamedPipeServer;
            type Addr = ();

            async fn accept(&mut self) -> (Self::Io, Self::Addr) {
                self.next.connect().await.unwrap();
                let next = ServerOptions::new().create(&self.path).unwrap();
                (std::mem::replace(&mut self.next, next), ())
            }

            fn local_addr(&self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let path = format!(r"\\.\pipe\nyanpasu-app-test-{}", uuid::Uuid::new_v4());
        let listener = PipeListener {
            next: ServerOptions::new()
                .first_pipe_instance(true)
                .create(&path)
                .unwrap(),
            path: path.clone(),
        };
        let (closed, mut closed_rx) = mpsc::unbounded_channel();
        let router = Router::new()
            .route("/traffic", get(idle))
            .route(
                "/configs",
                get(|| async { Json(serde_json::json!({"mode":"rule"})) }),
            )
            .with_state(closed);
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut endpoint = endpoint("http://127.0.0.1:1/".into());
        std::sync::Arc::get_mut(&mut endpoint).unwrap().host =
            nyanpasu_application::core::endpoint::ExecutionHost::Service;
        endpoint.binding.send_modify(|binding| {
            binding.as_mut().unwrap().controller = CoreControllerInfo::NamedPipe(path.into());
        });
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            api.configs().await.unwrap();
            let mut stream = api.traffic_ws().await.unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap().up.get(), 1);
            endpoint.binding.send_replace(None);
            assert!(matches!(stream.next().await, Some(Err(ApiError::Stale))));
            closed_rx.recv().await.unwrap();
        })
        .await
        .unwrap();
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn websocket_keeps_hot_patch_binding_and_releases_idle_socket_on_revocation() {
        for change in ["instance", "secret", "controller", "shutdown"] {
            let (closed, mut rx) = mpsc::unbounded_channel();
            let (url, server) = server(
                Router::new()
                    .route("/traffic", get(idle))
                    .with_state(closed),
            )
            .await;
            let endpoint = endpoint(url.clone());
            let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
            let api = core.api_client().await.unwrap();
            let mut stream = api.traffic_ws().await.unwrap();
            endpoint.binding.send_modify(|_| {});
            assert!(core.api_client().await.unwrap().same_instance(&api));
            assert_eq!(stream.next().await.unwrap().unwrap().up.get(), 1);
            if change == "shutdown" {
                core.shutdown().await.unwrap();
            } else {
                endpoint.binding.send_modify(|binding| {
                    let binding = binding.as_mut().unwrap();
                    match change {
                        "instance" => binding.instance_id = "replacement".into(),
                        "secret" => binding.secret = Some("changed".into()),
                        "controller" => {
                            binding.controller =
                                CoreControllerInfo::Http(format!("{url}replacement/"))
                        }
                        _ => unreachable!(),
                    }
                });
                core.api_client().await.unwrap();
            }
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), stream.next())
                    .await
                    .unwrap(),
                Some(Err(ApiError::Stale))
            ));
            assert!(stream.next().await.is_none());
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap();
            server.abort();
        }
    }
}

#[cfg(test)]
mod proxy_snapshot_tests {
    use super::{
        tests::{endpoint, server},
        *,
    };
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
    use nyanpasu_application::core::CoreClient;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn router(
        group: impl Fn(usize) -> axum::response::Response + Clone + Send + Sync + 'static,
    ) -> (Router, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        let router = Router::new()
            .route(
                "/proxies",
                get(|| async { Json(serde_json::json!({"proxies":{}})) }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(serde_json::json!({"providers":{}})) }),
            )
            .route(
                "/group",
                get(move || {
                    let response = group(counter.fetch_add(1, Ordering::SeqCst));
                    async move { response }
                }),
            );
        (router, reads)
    }

    #[tokio::test]
    async fn missing_group_route_is_probed_once_per_instance() {
        let (router, reads) = router(|_| StatusCode::NOT_FOUND.into_response());
        let (url, server) = server(router).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        for _ in 0..2 {
            assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);

        endpoint.binding.send_modify(|binding| {
            binding.as_mut().unwrap().instance_id = "second-process".into();
        });
        let api = core.api_client().await.unwrap();
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn transient_group_failures_fall_back_until_the_list_answers() {
        let (router, reads) = router(|attempt| match attempt {
            0 => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            1 => Json(serde_json::json!({"proxies":{
                "Meow": {"name":"Meow","type":"Selector","udp":true,"history":[],"all":[]}
            }}))
            .into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        });
        let (url, server) = server(router).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let api = core.api_client().await.unwrap();
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        let groups = api.proxy_snapshot().await.unwrap().groups.unwrap();
        assert!(groups.contains_key(&ProxyName::from("Meow")));
        // A later failure no longer changes what the instance supports.
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        core.shutdown().await.unwrap();
        server.abort();
    }
}

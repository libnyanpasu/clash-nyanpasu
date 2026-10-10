use std::{sync::Arc, time::Duration};

use crate::{
    control::endpoint::{
        ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    },
    logs::{
        CoreLogCursor, CoreLogPage, CoreLogQuery, CoreLogRecord, CoreLogResult, CoreLogStatus,
        CoreLogStore, CoreLogsClient, PreparedCoreLog, RedbCoreLogStore,
    },
};
use axum::Router;
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo, OperationOutputInfo, OperationPhase},
    status::{CoreControllerInfo, CoreStateDetail},
};
use tempfile::TempDir;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

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

struct TestStore {
    // Field drop order keeps the directory until the database has closed.
    inner: RedbCoreLogStore,
    _directory: TempDir,
}
impl CoreLogStore for TestStore {
    fn configure(
        &mut self,
        settings: nyanpasu_config::application::CoreLogSettings,
    ) -> CoreLogResult<()> {
        self.inner.configure(settings)
    }
    fn append(&mut self, records: &[PreparedCoreLog]) -> CoreLogResult<()> {
        self.inner.append(records)
    }
    fn query(&mut self, request: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.query(request)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.detail(cursor)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.inner.clear()
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.inner.status()
    }
}

pub(crate) async fn core_logs_client() -> CoreLogsClient {
    let directory = TempDir::new().unwrap();
    let store = TestStore {
        inner: RedbCoreLogStore::open(directory.path().into()).unwrap(),
        _directory: directory,
    };
    let client = CoreLogsClient::spawn(
        Box::new(store),
        Default::default(),
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    client.set_instance(Some("instance".into())).await.unwrap();
    client
}

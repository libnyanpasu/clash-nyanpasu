//! The local-socket transport for the CLI: length-prefixed JSON frames over a
//! Unix domain socket. Socket permissions are its only authentication, so the
//! server keeps the socket owner-only; the registry allows only commands that
//! opted into [`RpcTarget::Local`].
//!
//! Each accepted connection is one owner. A connection may carry several
//! requests, which run concurrently and are answered by id; a call keeps
//! running when its connection goes away.

use std::path::{Path, PathBuf};

use nyanpasu_rpc_protocol::{
    RpcError, RpcErrorKind, RpcOwner, RpcTarget,
    frame::{self, FrameError, LocalRequest, LocalResponse, PROTOCOL_VERSION},
};
use serde_json::Value;
use snafu::Snafu;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::UnifiedRpc;

#[derive(Debug, Snafu)]
pub enum LocalServeError {
    #[snafu(display("failed to create the socket directory {}", path.display()))]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("{} exists and is not a socket", path.display()))]
    NotASocket { path: PathBuf },
    #[snafu(display("another server already listens on {}", path.display()))]
    AlreadyServing { path: PathBuf },
    #[snafu(display("failed to inspect or replace {}", path.display()))]
    ReplaceStale {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to bind {}", path.display()))]
    Bind {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to restrict {} to its owner", path.display()))]
    Restrict {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("the local socket needs Unix domain sockets"))]
    Unsupported,
}

/// Serves `rpc` on a Unix socket at `path` until `shutdown` is cancelled,
/// then waits for the calls in flight and removes the socket.
///
/// A missing parent directory is created owner-only (`0700`); an existing one
/// is the caller's to secure. The socket itself is `0600`. A socket left by a
/// server that is gone is replaced; one that still answers is not.
///
/// # Errors
///
/// Fails when the socket cannot be set up; accept errors are logged and
/// serving continues.
#[cfg(unix)]
pub async fn serve_local(
    path: &Path,
    rpc: UnifiedRpc,
    shutdown: CancellationToken,
) -> Result<(), LocalServeError> {
    let listener = bind(path).await?;
    let tasks = TaskTracker::new();
    let mut connections = 0u64;
    loop {
        let accepted = tokio::select! {
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((stream, _)) => {
                connections += 1;
                tasks.spawn(serve_stream(
                    stream,
                    RpcOwner::local_connection(connections),
                    rpc.clone(),
                    shutdown.clone(),
                    tasks.clone(),
                ));
            }
            Err(error) => tracing::warn!(%error, "failed to accept a local RPC connection"),
        }
    }
    drop(listener);
    if let Err(error) = tokio::fs::remove_file(path).await {
        tracing::warn!(%error, path = %path.display(), "failed to remove the local RPC socket");
    }
    tasks.close();
    tasks.wait().await;
    Ok(())
}

/// The local socket is a Unix domain socket; other platforms do not offer it.
///
/// # Errors
///
/// Always fails with [`LocalServeError::Unsupported`].
#[cfg(not(unix))]
pub async fn serve_local(
    _path: &Path,
    _rpc: UnifiedRpc,
    _shutdown: CancellationToken,
) -> Result<(), LocalServeError> {
    UnsupportedSnafu.fail()
}

#[cfg(unix)]
async fn bind(path: &Path) -> Result<tokio::net::UnixListener, LocalServeError> {
    use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};

    use snafu::ResultExt as _;

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !tokio::fs::try_exists(directory)
        .await
        .context(CreateDirectorySnafu { path: directory })?
    {
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .await
            .context(CreateDirectorySnafu { path: directory })?;
    }
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            snafu::ensure!(metadata.file_type().is_socket(), NotASocketSnafu { path });
            snafu::ensure!(
                tokio::net::UnixStream::connect(path).await.is_err(),
                AlreadyServingSnafu { path }
            );
            tokio::fs::remove_file(path)
                .await
                .context(ReplaceStaleSnafu { path })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(LocalServeError::ReplaceStale {
                path: path.into(),
                source,
            });
        }
    }
    // Bind under a private name, restrict it, then move it into place, so the
    // published path never accepts a connection before it is owner-only.
    let mut binding = path.as_os_str().to_owned();
    binding.push(".binding");
    let binding = PathBuf::from(binding);
    if let Err(source) = tokio::fs::remove_file(&binding).await
        && source.kind() != std::io::ErrorKind::NotFound
    {
        return Err(LocalServeError::ReplaceStale {
            path: binding,
            source,
        });
    }
    let listener = tokio::net::UnixListener::bind(&binding).context(BindSnafu { path })?;
    tokio::fs::set_permissions(&binding, std::fs::Permissions::from_mode(0o600))
        .await
        .context(RestrictSnafu { path })?;
    tokio::fs::rename(&binding, path)
        .await
        .context(BindSnafu { path })?;
    Ok(listener)
}

/// Serves one connection until it closes or `shutdown` is cancelled. Calls in
/// flight run on `tasks` and outlive the connection.
#[cfg_attr(
    all(not(unix), not(test)),
    expect(
        dead_code,
        reason = "only the Unix listener serves streams outside tests"
    )
)]
pub(crate) async fn serve_stream<S>(
    stream: S,
    owner: RpcOwner,
    rpc: UnifiedRpc,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (replies, mut outgoing) = mpsc::unbounded_channel::<LocalResponse>();
    // The writer ends once the reader and every call in flight drop their sender.
    tasks.spawn(async move {
        while let Some(response) = outgoing.recv().await {
            let frame = frame::encode(&response).unwrap_or_else(|error| {
                let error = RpcError::new(
                    RpcErrorKind::ApplicationError,
                    format!("the reply could not be sent: {error}"),
                );
                frame::encode(&LocalResponse::new(response.id, Err(error)))
                    .expect("an error reply fits in a frame")
            });
            if frame::write_frame(&mut writer, &frame).await.is_err() {
                break;
            }
        }
    });
    loop {
        let body = tokio::select! {
            () = shutdown.cancelled() => break,
            body = frame::read_frame(&mut reader) => body,
        };
        let body = match body {
            Ok(Some(body)) => body,
            Ok(None) => break,
            Err(FrameError::TooLarge { len }) => {
                // The body cannot be skipped safely, so the connection ends here.
                let error = RpcError::invalid_request(format!(
                    "a {len}-byte frame exceeds the {}-byte limit",
                    frame::MAX_FRAME_LEN
                ));
                let _ = replies.send(LocalResponse::new(None, Err(error)));
                break;
            }
            Err(error) => {
                tracing::debug!(%error, "local RPC connection failed");
                break;
            }
        };
        let request = match parse_request(&body) {
            Ok(request) => request,
            Err((id, error)) => {
                let _ = replies.send(LocalResponse::new(id, Err(error)));
                continue;
            }
        };
        let (rpc, owner, replies) = (rpc.clone(), owner.clone(), replies.clone());
        let call_tasks = tasks.clone();
        tasks.spawn(async move {
            let LocalRequest {
                id, method, params, ..
            } = request;
            // The command runs on its own task so that a panic in it ends only
            // that task and the caller still gets a reply instead of waiting.
            let reply = match call_tasks
                .spawn({
                    let method = method.clone();
                    async move { rpc.call(RpcTarget::Local, &method, owner, params).await }
                })
                .await
            {
                Ok(reply) => reply,
                Err(error) => {
                    tracing::error!(%error, method, "a local RPC command ended without a result");
                    Err(RpcError::new(
                        RpcErrorKind::Internal,
                        format!("command `{method}` stopped unexpectedly: {error}"),
                    ))
                }
            };
            // A caller that left gets no reply; the call has completed anyway.
            let _ = replies.send(LocalResponse::new(Some(id), reply));
        });
    }
}

#[cfg_attr(
    all(not(unix), not(test)),
    expect(
        dead_code,
        reason = "only the Unix listener serves streams outside tests"
    )
)]
fn parse_request(body: &[u8]) -> Result<LocalRequest, (Option<u64>, RpcError)> {
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        (
            None,
            RpcError::invalid_request(format!("malformed frame: {error}")),
        )
    })?;
    let id = value.get("id").and_then(Value::as_u64);
    let version = value.get("v").and_then(Value::as_u64);
    if version != Some(u64::from(PROTOCOL_VERSION)) {
        return Err((
            id,
            RpcError::invalid_request(format!(
                "unsupported protocol version {}; this server speaks {PROTOCOL_VERSION}",
                value.get("v").unwrap_or(&Value::Null)
            )),
        ));
    }
    serde_json::from_value(value).map_err(|error| {
        (
            id,
            RpcError::invalid_request(format!("malformed request: {error}")),
        )
    })
}

#[cfg(test)]
mod tests;

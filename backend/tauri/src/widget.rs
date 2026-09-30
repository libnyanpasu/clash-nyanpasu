use crate::client::{
    effects::status::failure_text,
    ui_effects::ports::{
        ConnectWidgetSnafu, CreateIpcServerSnafu, DuplicateStdioSnafu, LocateExecutableSnafu,
        MissingWidgetSenderSnafu, ResolveStatePathSnafu, ShutdownBeforeConnectSnafu,
        ShuttingDownSnafu, SpawnWidgetSnafu, StopPreviousSnafu, WIDGET_STOP_BOUND, WaitWidgetSnafu,
        WidgetError, WidgetExitedSnafu,
    },
};
use futures_util::StreamExt;
use nyanpasu_traffic::TrafficSummary;

use nyanpasu_egui::{
    ipc::{
        IpcSender, Message, StatisticMessage, WidgetIpcError, create_ipc_server, release_server,
        send_message,
    },
    widget::StatisticWidgetVariant,
};
use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu, ensure};
use std::{
    sync::{Arc, Mutex as StdMutex, atomic::AtomicBool},
    time::Duration,
};
use tauri::utils::platform::current_exe;
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How long a running widget gets to leave on its own after `Stop`.
const STOP_GRACE: Duration = Duration::from_millis(500);

/// The OS side of one widget: its process, the blocking IPC handshake, and the
/// self-connect that releases a handshake nobody is going to complete. A seam,
/// so the ownership rules below are testable without the widget binary.
pub(crate) trait WidgetHost: Send + Sync + 'static {
    /// Spawns the widget process and opens the one-shot server it connects
    /// back to.
    fn spawn(&self, variant: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError>;
    /// Connects to a pending one-shot server as its client, so a handshake
    /// blocked in `accept` returns.
    fn release(&self, server_name: &str) -> Result<(), WidgetIpcError>;
}

pub(crate) struct SpawnedWidget {
    pub process: Box<dyn WidgetProcess>,
    /// Blocks until the widget, or `release`, connects to the server.
    pub handshake: Box<dyn FnOnce() -> Result<Box<dyn WidgetLink>, WidgetError> + Send>,
    pub server_name: String,
}

#[async_trait::async_trait]
pub(crate) trait WidgetProcess: Send + 'static {
    /// The exit status once the process has exited, `None` while it runs. An
    /// error means the status could not be read, which proves nothing.
    fn try_exit(&mut self) -> std::io::Result<Option<String>>;
    /// Resolves once the process has exited, describing how.
    async fn exited(&mut self) -> std::io::Result<String>;
    /// Kills the process and reaps it.
    async fn kill(&mut self) -> std::io::Result<()>;
}

/// The parent's end of the widget's IPC channel. Sending may block.
pub(crate) trait WidgetLink: Send + 'static {
    fn send(&self, message: Message) -> Result<(), WidgetIpcError>;
}

/// Why a message could not reach the widget. Only logged: a widget that
/// missed one sample shows the next.
#[derive(Debug, Snafu)]
pub(crate) enum WidgetSendError {
    #[snafu(display("the widget link is poisoned"))]
    LinkPoisoned,
    #[snafu(display("could not send the message to the widget"))]
    SendToWidget { source: WidgetIpcError },
}

/// Shared with the blocking thread a send runs on, so a stuck send holds the
/// link and never the instance slot.
type SharedLink = Arc<StdMutex<Box<dyn WidgetLink>>>;

#[derive(Clone)]
pub struct WidgetManager {
    host: Arc<dyn WidgetHost>,
    instance: Arc<Mutex<Option<WidgetInstance>>>,
    listener_initd: Arc<AtomicBool>,
    /// Once cancelled, no widget starts, a handshake in progress ends, and
    /// the tracked stop reaps whatever is owned.
    shutdown: CancellationToken,
}

/// The widget this manager owns (T10 §5.6). It is recorded the moment its
/// process exists and leaves the slot only once its exit is confirmed, so a
/// start or a stop cancelled partway never takes the process with it.
enum WidgetInstance {
    /// Spawned and not yet connected back. The handshake worker blocks in
    /// `accept`; its handle is kept so a stop can release it and see it end.
    Starting {
        process: Box<dyn WidgetProcess>,
        worker: JoinHandle<()>,
        server_name: String,
    },
    Running {
        link: SharedLink,
        process: Box<dyn WidgetProcess>,
    },
}

impl WidgetManager {
    /// `tasks` tracks the stop that runs once `shutdown` is cancelled, so the
    /// shutdown waits until the owned widget is gone or its bound ran out.
    pub(crate) fn new(
        host: Arc<dyn WidgetHost>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Self {
        let manager = Self {
            host,
            instance: Arc::new(Mutex::new(None)),
            listener_initd: Arc::new(AtomicBool::new(false)),
            shutdown,
        };
        let stopping = manager.clone();
        tasks.spawn(async move {
            stopping.shutdown.cancelled().await;
            if let Err(error) = stopping.stop(Instant::now() + WIDGET_STOP_BOUND).await {
                tracing::warn!("the widget was not stopped on the way out: {error}");
            }
        });
        manager
    }

    fn register_listener(&self, client: crate::client::NyanpasuClient) {
        if self
            .listener_initd
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let signal = self.listener_initd.clone();
        let this = self.clone();
        tokio::spawn(async move {
            let mut receiver = crate::core::clash::traffic::summary_stream(client);
            loop {
                let received = tokio::select! {
                    received = receiver.next() => received,
                    () = this.shutdown.cancelled() => break,
                };
                match received {
                    Some(Ok(event)) => {
                        if let Err(error) = this.handle_event(event).await {
                            tracing::warn!(%error,"failed to update widget traffic");
                        }
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error,"widget traffic subscription unavailable");
                        if let Err(error) = this.handle_event(None).await {
                            tracing::warn!(%error,"failed to clear widget traffic");
                        }
                    }
                    None => {
                        signal.store(false, std::sync::atomic::Ordering::Release);
                        break;
                    }
                }
            }
        });
        self.listener_initd
            .store(true, std::sync::atomic::Ordering::Release);
    }

    async fn handle_event(&self, event: Option<TrafficSummary>) -> Result<(), WidgetSendError> {
        // The widget protocol has numeric display fields; unknown rates render
        // as zero here while the traffic domain retains its unknown state.
        let rate = event.as_ref().and_then(|event| event.current_rate.as_ref());
        let statistic = StatisticMessage {
            download_total: event
                .as_ref()
                .map_or(0, |event| event.session.core_reported_bytes.download.0),
            upload_total: event
                .as_ref()
                .map_or(0, |event| event.session.core_reported_bytes.upload.0),
            download_speed: rate.map_or(0, |rate| rate.download as u64),
            upload_speed: rate.map_or(0, |rate| rate.upload as u64),
        };
        let link = {
            let mut instance = self.instance.lock().await;
            let Some(WidgetInstance::Running { link, process }) = instance.as_mut() else {
                return Ok(());
            };
            if has_exited(process) {
                return Ok(());
            }
            link.clone()
        };
        crate::utils::blocking::join(
            tokio::task::spawn_blocking(move || send(&link, Message::UpdateStatistic(statistic)))
                .await,
        )
    }

    pub async fn start(&self, widget: StatisticWidgetVariant) -> Result<(), WidgetError> {
        // Held for the whole start, so a stop never finds the slot about to
        // fill behind its back.
        let mut instance = self.instance.lock().await;
        ensure!(!self.shutdown.is_cancelled(), ShuttingDownSnafu);
        if instance.is_some() {
            log::info!("Widget already running, stopping it first...");
            self.stop_owned(&mut instance, Instant::now() + WIDGET_STOP_BOUND)
                .await
                .context(StopPreviousSnafu)?;
        }
        let spawned = self.host.spawn(widget)?;
        tracing::debug!("Waiting for widget process to start...");
        let (linked_tx, linked_rx) = oneshot::channel();
        let handshake = spawned.handshake;
        // Recorded before the first await: from here on, dropping this future
        // leaves the process and its handshake worker owned by the manager.
        instance.replace(WidgetInstance::Starting {
            process: spawned.process,
            worker: tokio::task::spawn_blocking(move || {
                let _ = linked_tx.send(handshake());
            }),
            server_name: spawned.server_name,
        });
        let Some(WidgetInstance::Starting { process, .. }) = instance.as_mut() else {
            unreachable!("the starting instance was recorded above");
        };
        let linked = tokio::select! {
            // The worker drops the sender unsent only by unwinding.
            linked = linked_rx => linked
                .unwrap_or_else(|_| panic!("the widget handshake worker panicked")),
            exited = process.exited() => match exited {
                Ok(status) => WidgetExitedSnafu { status }.fail(),
                Err(source) => Err(WaitWidgetSnafu.into_error(source)),
            },
            // The handshake has no bound of its own; the stop below reaps
            // the child and releases the worker.
            () = self.shutdown.cancelled() => ShutdownBeforeConnectSnafu.fail(),
        };
        match linked {
            Ok(link) => {
                if let Some(WidgetInstance::Starting { process, .. }) = instance.take() {
                    instance.replace(WidgetInstance::Running {
                        link: Arc::new(StdMutex::new(link)),
                        process,
                    });
                }
                Ok(())
            }
            Err(error) => {
                // What is left of the attempt stays owned until it is gone.
                if let Err(cleanup) = self
                    .stop_owned(&mut instance, Instant::now() + WIDGET_STOP_BOUND)
                    .await
                {
                    tracing::warn!("failed to clean up a widget that did not start: {cleanup}");
                }
                Err(error)
            }
        }
    }

    /// Stops the owned widget by `deadline`. The slot is cleared only once
    /// the exit is confirmed; otherwise the widget stays owned and the error
    /// says what is still outstanding.
    pub async fn stop(&self, deadline: Instant) -> Result<(), WidgetError> {
        let Ok(mut instance) = tokio::time::timeout_at(deadline, self.instance.lock()).await else {
            return Err(WidgetError::StillOwned);
        };
        self.stop_owned(&mut instance, deadline).await
    }

    async fn stop_owned(
        &self,
        slot: &mut Option<WidgetInstance>,
        deadline: Instant,
    ) -> Result<(), WidgetError> {
        match slot {
            None => {
                tracing::debug!("Widget instance is not exists, skipping...");
                return Ok(());
            }
            Some(WidgetInstance::Running { link, process }) => {
                if !has_exited(process) {
                    // first try to stop the process gracefully
                    let link = link.clone();
                    let sent = tokio::time::timeout_at(
                        deadline,
                        tokio::task::spawn_blocking(move || send(&link, Message::Stop)),
                    )
                    .await;
                    match sent {
                        Ok(Ok(Ok(()))) => {
                            let grace = deadline.min(Instant::now() + STOP_GRACE);
                            let _ = tokio::time::timeout_at(grace, process.exited()).await;
                        }
                        Ok(Ok(Err(error))) => {
                            tracing::warn!(
                                "failed to send stop message to widget: {}",
                                failure_text(&error)
                            )
                        }
                        Ok(Err(error)) => match error.try_into_panic() {
                            Ok(panic) => std::panic::resume_unwind(panic),
                            Err(error) => {
                                tracing::warn!("widget stop message task failed: {error}")
                            }
                        },
                        Err(_) => tracing::warn!("widget stop message did not go out in time"),
                    }
                }
                if !exit_confirmed(process, deadline).await {
                    return Err(WidgetError::StillOwned);
                }
            }
            Some(WidgetInstance::Starting {
                process,
                worker,
                server_name,
            }) => {
                if !exit_confirmed(process, deadline).await {
                    return Err(WidgetError::StillOwned);
                }
                // The dead child never connects, and its exit does not close
                // the listening end; only a connection of our own ends
                // `accept`.
                let host = self.host.clone();
                let name = server_name.clone();
                match tokio::time::timeout_at(
                    deadline,
                    tokio::task::spawn_blocking(move || host.release(&name)),
                )
                .await
                {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(error))) => {
                        tracing::warn!(
                            "failed to release the widget handshake: {}",
                            failure_text(&error)
                        )
                    }
                    Ok(Err(error)) => match error.try_into_panic() {
                        Ok(panic) => std::panic::resume_unwind(panic),
                        Err(error) => {
                            tracing::warn!("widget handshake release task failed: {error}")
                        }
                    },
                    Err(_) => tracing::warn!("widget handshake release did not finish in time"),
                }
                if tokio::time::timeout_at(deadline, worker).await.is_err() {
                    return Err(WidgetError::HandshakeBlocked);
                }
            }
        }
        *slot = None;
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        let mut instance = self.instance.lock().await;
        match instance.as_mut() {
            Some(WidgetInstance::Running { process, .. }) => !has_exited(process),
            _ => false,
        }
    }
}

/// Only a read exit status counts as an exit: a process whose status cannot
/// be read is treated as still running.
fn has_exited(process: &mut Box<dyn WidgetProcess>) -> bool {
    match process.try_exit() {
        Ok(status) => status.is_some(),
        Err(error) => {
            tracing::warn!("failed to read the widget process status: {error}");
            false
        }
    }
}

/// Kills the process unless it has exited, and says whether its exit is
/// confirmed by `deadline`.
async fn exit_confirmed(process: &mut Box<dyn WidgetProcess>, deadline: Instant) -> bool {
    if has_exited(process) {
        return true;
    }
    match tokio::time::timeout_at(deadline, process.kill()).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!("failed to kill widget process: {error}");
            has_exited(process)
        }
        Err(_) => false,
    }
}

fn send(link: &SharedLink, message: Message) -> Result<(), WidgetSendError> {
    #[cfg(debug_assertions)]
    tracing::debug!("Sending message to widget: {:?}", message);
    link.lock()
        .ok()
        .context(LinkPoisonedSnafu)?
        .send(message)
        .context(SendToWidgetSnafu)
}

/// The widget binary: this executable, relaunched with `statistic-widget`.
struct ProcessWidgetHost;

impl WidgetHost for ProcessWidgetHost {
    fn spawn(&self, widget: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError> {
        let current_exe = current_exe().context(LocateExecutableSnafu)?;
        // This operation is blocking, but it internal just a system call, so I think it's okay
        let (mut ipc_server, server_name) = create_ipc_server().context(CreateIpcServerSnafu)?;
        // spawn a process to run the widget
        let variant = format!("{widget}");
        tracing::debug!("Spawning widget process for {}...", variant);
        let widget_win_state_path = crate::utils::dirs::app_data_dir()
            .map_err(anyhow::Error::into)
            .context(ResolveStatePathSnafu)?
            .join(format!("widget_{variant}.state"));
        let child = tokio::process::Command::new(current_exe)
            .arg("statistic-widget")
            .arg(&variant)
            .env("NYANPASU_EGUI_IPC_SERVER", &server_name)
            .env("NYANPASU_EGUI_WINDOW_STATE_PATH", widget_win_state_path)
            .stdin(std::process::Stdio::inherit())
            .stdout(os_pipe::dup_stdout().context(DuplicateStdioSnafu)?)
            .stderr(os_pipe::dup_stderr().context(DuplicateStdioSnafu)?)
            // The last backstop: an instance that is dropped takes its
            // process with it.
            .kill_on_drop(true)
            .spawn()
            .context(SpawnWidgetSnafu { variant })?;
        Ok(SpawnedWidget {
            process: Box::new(child),
            handshake: Box::new(move || {
                ipc_server.connect().context(ConnectWidgetSnafu)?;
                let tx = ipc_server.into_tx().context(MissingWidgetSenderSnafu)?;
                Ok(Box::new(tx) as Box<dyn WidgetLink>)
            }),
            server_name,
        })
    }

    fn release(&self, server_name: &str) -> Result<(), WidgetIpcError> {
        release_server(server_name)
    }
}

#[async_trait::async_trait]
impl WidgetProcess for tokio::process::Child {
    fn try_exit(&mut self) -> std::io::Result<Option<String>> {
        Ok(self.try_wait()?.map(|status| status.to_string()))
    }

    async fn exited(&mut self) -> std::io::Result<String> {
        Ok(self.wait().await?.to_string())
    }

    async fn kill(&mut self) -> std::io::Result<()> {
        tokio::process::Child::kill(self).await
    }
}

impl WidgetLink for IpcSender<Message> {
    fn send(&self, message: Message) -> Result<(), WidgetIpcError> {
        send_message(self, message)
    }
}

/// Builds the widget manager and starts listening for traffic samples.
///
/// It no longer reads the configuration or starts the widget: which variant
/// should run is an application effect now, so the composition root installs
/// this into the widget controller and the startup reconcile hands it the
/// desired value. The manager stops the widget itself once `shutdown` is
/// cancelled; nothing stops it on drop.
pub async fn setup(
    client: crate::client::NyanpasuClient,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> anyhow::Result<WidgetManager> {
    let widget_manager = WidgetManager::new(Arc::new(ProcessWidgetHost), shutdown, tasks);
    widget_manager.register_listener(client);
    Ok(widget_manager)
}

#[cfg(test)]
impl WidgetManager {
    /// Which state the owned widget is in, if any.
    pub(crate) async fn owned(&self) -> Option<&'static str> {
        self.instance
            .lock()
            .await
            .as_ref()
            .map(|instance| match instance {
                WidgetInstance::Starting { .. } => "starting",
                WidgetInstance::Running { .. } => "running",
            })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::Ordering;

    use tokio::sync::{Notify, watch};

    use super::*;

    pub(crate) type Log = Arc<StdMutex<Vec<&'static str>>>;

    fn record(log: &Log, event: &'static str) {
        log.lock().unwrap().push(event);
    }

    enum Handshake {
        Connect,
        Release,
    }

    /// A widget host with no binary behind it. Its handshake blocks until the
    /// widget connects or `release` connects in its place, and every OS-side
    /// step lands in `log`.
    pub(crate) struct FakeWidgetHost {
        pub log: Log,
        pub spawned: Notify,
        /// Notified on every `release`, reached or refused.
        pub released: Notify,
        connects: bool,
        releases: AtomicBool,
        kill_hangs: bool,
        exits_on_stop: bool,
        status_unreadable: Arc<AtomicBool>,
        gate: StdMutex<Option<std::sync::mpsc::Sender<Handshake>>>,
        process: StdMutex<Option<Arc<watch::Sender<bool>>>>,
    }

    impl FakeWidgetHost {
        fn new(connects: bool, kill_hangs: bool, exits_on_stop: bool) -> Arc<Self> {
            Arc::new(Self {
                log: Log::default(),
                spawned: Notify::new(),
                released: Notify::new(),
                connects,
                releases: AtomicBool::new(true),
                kill_hangs,
                exits_on_stop,
                status_unreadable: Arc::default(),
                gate: StdMutex::new(None),
                process: StdMutex::new(None),
            })
        }

        /// The widget never connects back; `release` unblocks the handshake.
        pub(crate) fn blocking() -> Arc<Self> {
            Self::new(false, false, false)
        }

        /// The widget connects at once and leaves when told to stop.
        pub(crate) fn connecting() -> Arc<Self> {
            Self::new(true, false, true)
        }

        /// The widget connects at once, ignores `Stop`, and cannot be killed.
        fn unkillable() -> Arc<Self> {
            Self::new(true, true, false)
        }

        /// From now on no process status can be read.
        fn fail_status_reads(&self) {
            self.status_unreadable.store(true, Ordering::SeqCst);
        }

        /// From now on `release` reaches nothing.
        pub(crate) fn refuse_release(&self) {
            self.releases.store(false, Ordering::SeqCst);
        }

        /// Ends a blocked handshake as `release` would have.
        pub(crate) fn unblock(&self) {
            if let Some(gate) = self.gate.lock().unwrap().as_ref() {
                let _ = gate.send(Handshake::Release);
            }
        }

        /// The last spawned process dies on its own.
        pub(crate) fn exit(&self) {
            if let Some(alive) = self.process.lock().unwrap().as_ref() {
                alive.send_replace(false);
            }
        }

        pub(crate) fn events(&self) -> Vec<&'static str> {
            self.log.lock().unwrap().clone()
        }
    }

    impl WidgetHost for FakeWidgetHost {
        fn spawn(&self, _: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError> {
            record(&self.log, "spawn");
            let (alive, observed) = watch::channel(true);
            let alive = Arc::new(alive);
            let (gate, handshake) = std::sync::mpsc::channel();
            if self.connects {
                let _ = gate.send(Handshake::Connect);
            }
            *self.gate.lock().unwrap() = Some(gate);
            *self.process.lock().unwrap() = Some(alive.clone());
            let link = FakeLink {
                log: self.log.clone(),
                alive: self.exits_on_stop.then(|| alive.clone()),
            };
            let log = self.log.clone();
            self.spawned.notify_one();
            Ok(SpawnedWidget {
                process: Box::new(FakeProcess {
                    log: self.log.clone(),
                    alive,
                    observed,
                    kill_hangs: self.kill_hangs,
                    status_unreadable: self.status_unreadable.clone(),
                }),
                handshake: Box::new(move || match handshake.recv() {
                    Ok(Handshake::Connect) => Ok(Box::new(link) as Box<dyn WidgetLink>),
                    Ok(Handshake::Release) | Err(_) => {
                        record(&log, "handshake released");
                        Err(WidgetError::ConnectWidget {
                            source: WidgetIpcError::AlreadyAccepted,
                        })
                    }
                }),
                server_name: "fake-server".into(),
            })
        }

        fn release(&self, server_name: &str) -> Result<(), WidgetIpcError> {
            assert_eq!(server_name, "fake-server");
            record(&self.log, "release");
            if self.releases.load(Ordering::SeqCst) {
                self.unblock();
            }
            self.released.notify_one();
            Ok(())
        }
    }

    struct FakeProcess {
        log: Log,
        alive: Arc<watch::Sender<bool>>,
        observed: watch::Receiver<bool>,
        kill_hangs: bool,
        status_unreadable: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl WidgetProcess for FakeProcess {
        fn try_exit(&mut self) -> std::io::Result<Option<String>> {
            if self.status_unreadable.load(Ordering::SeqCst) {
                return Err(std::io::Error::other(
                    "scripted: the process status cannot be read",
                ));
            }
            Ok((!*self.observed.borrow()).then(|| "exited".to_owned()))
        }

        async fn exited(&mut self) -> std::io::Result<String> {
            let _ = self.observed.wait_for(|alive| !alive).await;
            Ok("exited".into())
        }

        async fn kill(&mut self) -> std::io::Result<()> {
            record(&self.log, "kill");
            if self.kill_hangs {
                std::future::pending::<()>().await;
            }
            self.alive.send_replace(false);
            Ok(())
        }
    }

    struct FakeLink {
        log: Log,
        /// Set when the widget leaves on `Stop`.
        alive: Option<Arc<watch::Sender<bool>>>,
    }

    impl WidgetLink for FakeLink {
        fn send(&self, message: Message) -> Result<(), WidgetIpcError> {
            if matches!(message, Message::Stop) {
                record(&self.log, "stop message");
                if let Some(alive) = &self.alive {
                    alive.send_replace(false);
                }
            }
            Ok(())
        }
    }

    fn manager(host: &Arc<FakeWidgetHost>) -> WidgetManager {
        WidgetManager::new(host.clone(), CancellationToken::new(), &TaskTracker::new())
    }

    fn within(duration: Duration) -> Instant {
        Instant::now() + duration
    }

    #[tokio::test]
    async fn a_running_widget_leaves_on_stop_before_any_kill() {
        let host = FakeWidgetHost::connecting();
        let manager = manager(&host);
        manager.start(StatisticWidgetVariant::Small).await.unwrap();
        assert_eq!(manager.owned().await, Some("running"));
        assert!(manager.is_running().await);

        manager.stop(within(Duration::from_secs(5))).await.unwrap();

        assert_eq!(manager.owned().await, None);
        assert_eq!(host.events(), ["spawn", "stop message"]);
    }

    /// X5c: a start dropped mid-handshake — an aborted effects group — leaves
    /// the widget `Starting`, and the stop kills the child, releases the
    /// handshake and waits for the worker before it lets go of either.
    #[tokio::test]
    async fn a_start_cancelled_mid_handshake_stays_owned_until_the_stop_releases_it() {
        let host = FakeWidgetHost::blocking();
        let manager = manager(&host);
        let start = tokio::spawn({
            let manager = manager.clone();
            async move { manager.start(StatisticWidgetVariant::Small).await }
        });
        host.spawned.notified().await;
        // The start records the instance before its first await, so once it
        // yields the process is owned; aborting it drops only the waiter.
        tokio::task::yield_now().await;
        start.abort();
        assert!(start.await.unwrap_err().is_cancelled());
        assert_eq!(manager.owned().await, Some("starting"));
        assert!(!manager.is_running().await, "a widget that never connected");

        manager.stop(within(Duration::from_secs(5))).await.unwrap();

        assert_eq!(manager.owned().await, None);
        assert_eq!(
            host.events(),
            ["spawn", "kill", "release", "handshake released"],
            "the worker is seen to end before the slot is cleared"
        );
    }

    /// X5c: a release that reaches nothing leaves the worker blocked. The
    /// stop says so and keeps the handle, instead of claiming the worker went
    /// with the child.
    #[tokio::test]
    async fn a_refused_release_keeps_the_blocked_worker_owned_and_says_so() {
        let host = FakeWidgetHost::blocking();
        host.refuse_release();
        let manager = manager(&host);
        let start = tokio::spawn({
            let manager = manager.clone();
            async move { manager.start(StatisticWidgetVariant::Small).await }
        });
        host.spawned.notified().await;
        tokio::task::yield_now().await;
        start.abort();
        let _ = start.await;

        let error = manager
            .stop(within(Duration::from_millis(200)))
            .await
            .unwrap_err();

        assert!(matches!(error, WidgetError::HandshakeBlocked), "{error:?}");
        assert_eq!(error.to_string(), "widget handshake worker still blocked");
        assert_eq!(manager.owned().await, Some("starting"));
        assert_eq!(host.events(), ["spawn", "kill", "release"]);

        // Once the handshake does end, a later stop reaps it.
        host.unblock();
        manager.stop(within(Duration::from_secs(5))).await.unwrap();
        assert_eq!(manager.owned().await, None);
    }

    /// X5d: a stop that runs out of time, by its own deadline or by its
    /// caller giving up, leaves the widget in the slot.
    #[tokio::test(start_paused = true)]
    async fn a_stop_cut_short_keeps_the_widget_owned() {
        let host = FakeWidgetHost::unkillable();
        let manager = manager(&host);
        manager.start(StatisticWidgetVariant::Large).await.unwrap();

        let error = manager
            .stop(within(Duration::from_secs(3)))
            .await
            .unwrap_err();
        assert!(matches!(error, WidgetError::StillOwned), "{error:?}");
        assert_eq!(
            error.to_string(),
            "widget process still owned, exit not confirmed"
        );
        assert_eq!(manager.owned().await, Some("running"));

        let dropped = tokio::time::timeout(
            Duration::from_secs(1),
            manager.stop(within(Duration::from_secs(60))),
        )
        .await;
        assert!(dropped.is_err(), "the caller gave up first");
        assert_eq!(manager.owned().await, Some("running"));
    }

    /// A status that cannot be read is no evidence of an exit: the widget is
    /// still killed, and it is let go only once the kill confirms.
    #[tokio::test]
    async fn an_unreadable_status_is_not_taken_for_an_exit() {
        let host = FakeWidgetHost::connecting();
        let manager = manager(&host);
        manager.start(StatisticWidgetVariant::Small).await.unwrap();
        host.fail_status_reads();

        manager.stop(within(Duration::from_secs(5))).await.unwrap();

        assert_eq!(manager.owned().await, None);
        assert_eq!(host.events(), ["spawn", "stop message", "kill"]);
    }

    /// And when that kill never confirms, the widget stays owned.
    #[tokio::test(start_paused = true)]
    async fn an_unreadable_status_and_an_unconfirmed_kill_keep_the_widget_owned() {
        let host = FakeWidgetHost::unkillable();
        let manager = manager(&host);
        manager.start(StatisticWidgetVariant::Small).await.unwrap();
        host.fail_status_reads();

        let error = manager
            .stop(within(Duration::from_secs(3)))
            .await
            .unwrap_err();

        assert!(matches!(error, WidgetError::StillOwned), "{error:?}");
        assert_eq!(manager.owned().await, Some("running"));
    }

    #[tokio::test]
    async fn a_widget_that_exits_during_the_handshake_releases_its_worker() {
        let host = FakeWidgetHost::blocking();
        let manager = manager(&host);
        let start = tokio::spawn({
            let manager = manager.clone();
            async move { manager.start(StatisticWidgetVariant::Small).await }
        });
        host.spawned.notified().await;
        tokio::task::yield_now().await;
        // The child dies before connecting: its exit wakes the start.
        host.exit();

        let error = start.await.unwrap().unwrap_err();

        assert!(matches!(error, WidgetError::WidgetExited { .. }), "{error}");
        assert_eq!(manager.owned().await, None);
        assert_eq!(
            host.events(),
            ["spawn", "release", "handshake released"],
            "nothing left to kill, and the worker the child never reached is released"
        );
    }
}

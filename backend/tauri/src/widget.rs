use super::core::clash::ws::ClashConnectionsConnectorEvent;
use crate::client::{
    effects::status::failure_text,
    ui_effects::ports::{
        ConnectWidgetSnafu, CreateIpcServerSnafu, DuplicateStdioSnafu, LocateExecutableSnafu,
        ShutdownBeforeConnectSnafu, ShuttingDownSnafu, SpawnWidgetSnafu, StopPreviousSnafu,
        WIDGET_STOP_BOUND, WaitWidgetSnafu, WidgetError, WidgetExitedSnafu,
    },
};

use futures::future::BoxFuture;
use nyanpasu_widget_protocol::{
    Logo, Message, StatisticMessage, StatisticWidgetVariant,
    ipc::{Bootstrap, WidgetIpcError, WidgetSender, accept, create_server, send_message},
};
use snafu::{IntoError as _, ResultExt as _, Snafu, ensure};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tauri::utils::platform::current_exe;
use tokio::{
    sync::{
        Mutex,
        broadcast::{Receiver as BroadcastReceiver, error::RecvError as BroadcastRecvError},
    },
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How long a running widget gets to leave on its own after `Stop`.
const STOP_GRACE: Duration = Duration::from_millis(500);

/// The OS side of one widget: its process and the channel it connects back
/// to. A seam, so the ownership rules below are testable without the widget
/// binary.
pub(crate) trait WidgetHost: Send + Sync + 'static {
    /// Spawns the widget process and opens the one-shot server it connects
    /// back to. Called inside the Tokio runtime.
    fn spawn(&self, variant: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError>;
}

pub(crate) struct SpawnedWidget {
    pub process: Box<dyn WidgetProcess>,
    /// Resolves once the widget connects. Dropping it stops listening, so a
    /// start that gives up leaves nothing waiting behind it.
    pub handshake: BoxFuture<'static, Result<Arc<dyn WidgetLink>, WidgetError>>,
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

/// The parent's end of the widget's IPC channel. A send waits until the
/// message is handed to the OS, which takes as long as a stuck widget leaves
/// the pipe full.
#[async_trait::async_trait]
pub(crate) trait WidgetLink: Send + Sync + 'static {
    async fn send(&self, message: Message) -> Result<(), WidgetIpcError>;
}

/// Why a message could not reach the widget. Only logged: a widget that
/// missed one sample shows the next.
#[derive(Debug, Snafu)]
pub(crate) enum WidgetSendError {
    #[snafu(display("could not send the message to the widget"))]
    SendToWidget { source: WidgetIpcError },
}

/// Cloned out of the slot for every send, so a stuck send holds the link and
/// never the instance slot.
type SharedLink = Arc<dyn WidgetLink>;

#[derive(Clone)]
pub struct WidgetManager {
    host: Arc<dyn WidgetHost>,
    instance: Arc<Mutex<Option<WidgetInstance>>>,
    /// The logo every started widget is sent once it connects.
    logo: Arc<std::sync::Mutex<Logo>>,
    listener_initd: Arc<AtomicBool>,
    /// Once cancelled, no widget starts, a handshake in progress ends, and
    /// the tracked stop reaps whatever is owned.
    shutdown: CancellationToken,
}

/// The widget this manager owns (T10 §5.6). It is recorded the moment its
/// process exists and leaves the slot only once its exit is confirmed, so a
/// start or a stop cancelled partway never takes the process with it.
enum WidgetInstance {
    /// Spawned and not yet connected back. The handshake lives in the start
    /// that waits for it and ends with that start.
    Starting { process: Box<dyn WidgetProcess> },
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
            logo: Arc::default(),
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

    fn register_listener(&self, mut receiver: BroadcastReceiver<ClashConnectionsConnectorEvent>) {
        if self
            .listener_initd
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let signal = self.listener_initd.clone();
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                let received = tokio::select! {
                    received = receiver.recv() => received,
                    () = this.shutdown.cancelled() => break,
                };
                match received {
                    Ok(event) => {
                        if let Err(e) = this.handle_event(event).await {
                            log::error!("Failed to handle event: {e}");
                        }
                    }
                    Err(e) => {
                        log::error!("Error receiving event: {e}");
                        if BroadcastRecvError::Closed == e {
                            signal.store(false, std::sync::atomic::Ordering::Release);
                            break;
                        }
                    }
                }
            }
        });
        self.listener_initd
            .store(true, std::sync::atomic::Ordering::Release);
    }

    async fn handle_event(
        &self,
        event: ClashConnectionsConnectorEvent,
    ) -> Result<(), WidgetSendError> {
        // we only care about the update event now
        let ClashConnectionsConnectorEvent::Update(info) = event else {
            return Ok(());
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
        send(
            &link,
            Message::UpdateStatistic(StatisticMessage {
                download_total: info.download_total,
                upload_total: info.upload_total,
                download_speed: info.download_speed,
                upload_speed: info.upload_speed,
            }),
        )
        .await
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
        // Recorded before the first await: from here on, dropping this future
        // leaves the process owned by the manager. The handshake goes with
        // the future, and with it the listening end.
        instance.replace(WidgetInstance::Starting {
            process: spawned.process,
        });
        let Some(WidgetInstance::Starting { process }) = instance.as_mut() else {
            unreachable!("the starting instance was recorded above");
        };
        let linked = tokio::select! {
            linked = spawned.handshake => linked,
            exited = process.exited() => match exited {
                Ok(status) => WidgetExitedSnafu { status }.fail(),
                Err(source) => Err(WaitWidgetSnafu.into_error(source)),
            },
            // The handshake has no bound of its own; the stop below reaps
            // the child.
            () = self.shutdown.cancelled() => ShutdownBeforeConnectSnafu.fail(),
        };
        match linked {
            Ok(link) => {
                if let Some(WidgetInstance::Starting { process }) = instance.take() {
                    instance.replace(WidgetInstance::Running {
                        link: link.clone(),
                        process,
                    });
                }
                // Before the start returns, so no logo set meanwhile can be
                // overtaken by this older one.
                let logo = self.logo.lock().unwrap().clone();
                if let Err(error) = send(&link, Message::UpdateLogo(logo)).await {
                    tracing::warn!("failed to send the logo to the widget: {error}");
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
                    let sent = tokio::time::timeout_at(deadline, send(link, Message::Stop)).await;
                    match sent {
                        Ok(Ok(())) => {
                            let grace = deadline.min(Instant::now() + STOP_GRACE);
                            let _ = tokio::time::timeout_at(grace, process.exited()).await;
                        }
                        Ok(Err(error)) => {
                            tracing::warn!(
                                "failed to send stop message to widget: {}",
                                failure_text(&error)
                            )
                        }
                        Err(_) => tracing::warn!("widget stop message did not go out in time"),
                    }
                }
                if !exit_confirmed(process, deadline).await {
                    return Err(WidgetError::StillOwned);
                }
            }
            Some(WidgetInstance::Starting { process }) => {
                if !exit_confirmed(process, deadline).await {
                    return Err(WidgetError::StillOwned);
                }
            }
        }
        *slot = None;
        Ok(())
    }

    /// Remembers `logo` and sends it to the running widget.
    pub async fn set_logo(&self, logo: Logo) {
        *self.logo.lock().unwrap() = logo.clone();
        let link = {
            let mut instance = self.instance.lock().await;
            let Some(WidgetInstance::Running { link, process }) = instance.as_mut() else {
                return;
            };
            if has_exited(process) {
                return;
            }
            link.clone()
        };
        if let Err(error) = send(&link, Message::UpdateLogo(logo)).await {
            tracing::warn!("failed to send the logo to the widget: {error}");
        }
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

async fn send(link: &SharedLink, message: Message) -> Result<(), WidgetSendError> {
    #[cfg(debug_assertions)]
    tracing::debug!("Sending message to widget: {:?}", message);
    link.send(message).await.context(SendToWidgetSnafu)
}

/// The widget binary: this executable, relaunched with `statistic-widget`.
struct ProcessWidgetHost;

impl WidgetHost for ProcessWidgetHost {
    fn spawn(&self, widget: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError> {
        let current_exe = current_exe().context(LocateExecutableSnafu)?;
        let (server, bootstrap) = create_server().context(CreateIpcServerSnafu)?;
        // spawn a process to run the widget
        let variant = format!("{widget}");
        tracing::debug!("Spawning widget process for {}...", variant);
        let child = tokio::process::Command::new(current_exe)
            .arg("statistic-widget")
            .arg(&variant)
            .env(Bootstrap::ENV, bootstrap.to_string())
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
            handshake: Box::pin(async move {
                let tx = accept(server).await.context(ConnectWidgetSnafu)?;
                Ok(Arc::new(tx) as Arc<dyn WidgetLink>)
            }),
        })
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

#[async_trait::async_trait]
impl WidgetLink for WidgetSender {
    async fn send(&self, message: Message) -> Result<(), WidgetIpcError> {
        send_message(self, message).await
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
    ws_connections_receiver: BroadcastReceiver<ClashConnectionsConnectorEvent>,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> anyhow::Result<WidgetManager> {
    let widget_manager = WidgetManager::new(Arc::new(ProcessWidgetHost), shutdown, tasks);
    widget_manager.register_listener(ws_connections_receiver);
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
    use std::sync::{Mutex as StdMutex, atomic::Ordering};

    use nyanpasu_widget_protocol::ipc;
    use tokio::sync::{Notify, watch};

    use super::*;

    pub(crate) type Log = Arc<StdMutex<Vec<&'static str>>>;

    fn record(log: &Log, event: &'static str) {
        log.lock().unwrap().push(event);
    }

    /// Records that a handshake ended without the widget connecting: the
    /// start that awaited it gave up and dropped it.
    struct HandshakeDropped(Log);

    impl Drop for HandshakeDropped {
        fn drop(&mut self) {
            record(&self.0, "handshake dropped");
        }
    }

    /// A widget host with no binary behind it. Its handshake either connects
    /// at once or waits until it is dropped, and every OS-side step lands in
    /// `log`.
    pub(crate) struct FakeWidgetHost {
        pub log: Log,
        pub spawned: Notify,
        connects: bool,
        kill_hangs: bool,
        exits_on_stop: bool,
        status_unreadable: Arc<AtomicBool>,
        process: StdMutex<Option<Arc<watch::Sender<bool>>>>,
    }

    impl FakeWidgetHost {
        fn new(connects: bool, kill_hangs: bool, exits_on_stop: bool) -> Arc<Self> {
            Arc::new(Self {
                log: Log::default(),
                spawned: Notify::new(),
                connects,
                kill_hangs,
                exits_on_stop,
                status_unreadable: Arc::default(),
                process: StdMutex::new(None),
            })
        }

        /// The widget never connects back.
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
            *self.process.lock().unwrap() = Some(alive.clone());
            let link = FakeLink {
                log: self.log.clone(),
                alive: self.exits_on_stop.then(|| alive.clone()),
            };
            let connects = self.connects;
            let dropped = HandshakeDropped(self.log.clone());
            self.spawned.notify_one();
            Ok(SpawnedWidget {
                process: Box::new(FakeProcess {
                    log: self.log.clone(),
                    alive,
                    observed,
                    kill_hangs: self.kill_hangs,
                    status_unreadable: self.status_unreadable.clone(),
                }),
                handshake: Box::pin(async move {
                    if !connects {
                        std::future::pending::<()>().await;
                    }
                    std::mem::forget(dropped);
                    Ok(Arc::new(link) as Arc<dyn WidgetLink>)
                }),
            })
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

    #[async_trait::async_trait]
    impl WidgetLink for FakeLink {
        async fn send(&self, message: Message) -> Result<(), WidgetIpcError> {
            if matches!(message, Message::Stop) {
                record(&self.log, "stop message");
                if let Some(alive) = &self.alive {
                    alive.send_replace(false);
                }
            }
            Ok(())
        }
    }

    /// A host whose widget is a thread on the far end of a real channel: the
    /// widget's own receiver loop, with a fake process that exits once that
    /// loop hands over `Stop`.
    struct ChannelWidgetHost {
        received: StdMutex<Option<std::sync::mpsc::Sender<Message>>>,
    }

    impl WidgetHost for ChannelWidgetHost {
        fn spawn(&self, _: StatisticWidgetVariant) -> Result<SpawnedWidget, WidgetError> {
            let (server, bootstrap) = ipc::create_server().context(CreateIpcServerSnafu)?;
            let (alive, observed) = watch::channel(true);
            let alive = Arc::new(alive);
            let received = self.received.lock().unwrap().take().unwrap();
            let exits = alive.clone();
            std::thread::spawn(move || {
                let rx = ipc::connect(&bootstrap).unwrap();
                ipc::spawn_receiver(rx, move |message| {
                    let stop = matches!(message, Message::Stop);
                    let _ = received.send(message);
                    if stop {
                        exits.send_replace(false);
                    }
                });
            });
            Ok(SpawnedWidget {
                process: Box::new(FakeProcess {
                    log: Log::default(),
                    alive,
                    observed,
                    kill_hangs: false,
                    status_unreadable: Arc::default(),
                }),
                handshake: Box::pin(async move {
                    let tx = ipc::accept(server).await.context(ConnectWidgetSnafu)?;
                    Ok(Arc::new(tx) as Arc<dyn WidgetLink>)
                }),
            })
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

    /// The manager's own messages reach the widget's receiver loop over a
    /// real channel, and `Stop` ends the widget. The logo set before the
    /// start arrives first, and one set later follows.
    #[tokio::test]
    async fn a_real_channel_carries_updates_and_stop_to_the_widget() {
        let (received, messages) = std::sync::mpsc::channel();
        let host = Arc::new(ChannelWidgetHost {
            received: StdMutex::new(Some(received)),
        });
        let manager = WidgetManager::new(host, CancellationToken::new(), &TaskTracker::new());
        let tun = Logo::Builtin(nyanpasu_widget_protocol::LogoPreset::Tun);
        manager.set_logo(tun.clone()).await;
        manager.start(StatisticWidgetVariant::Large).await.unwrap();
        assert!(manager.is_running().await);
        let image = Logo::Image(vec![0x89, b'P', b'N', b'G']);
        manager.set_logo(image.clone()).await;

        let link = match manager.instance.lock().await.as_ref() {
            Some(WidgetInstance::Running { link, .. }) => link.clone(),
            _ => unreachable!("the widget connected"),
        };
        send(&link, Message::UpdateStatistic(StatisticMessage::default()))
            .await
            .unwrap();
        drop(link);
        manager.stop(within(Duration::from_secs(5))).await.unwrap();

        assert!(matches!(messages.recv().unwrap(), Message::UpdateLogo(logo) if logo == tun));
        assert!(matches!(messages.recv().unwrap(), Message::UpdateLogo(logo) if logo == image));
        assert!(matches!(
            messages.recv().unwrap(),
            Message::UpdateStatistic(_)
        ));
        assert!(matches!(messages.recv().unwrap(), Message::Stop));
        assert_eq!(manager.owned().await, None);
    }

    /// X5c: a start dropped mid-handshake — an aborted effects group — leaves
    /// the widget `Starting` and stops listening, and the stop kills the
    /// child before it lets go of it.
    #[tokio::test]
    async fn a_start_cancelled_mid_handshake_stops_listening_and_stays_owned_until_the_stop() {
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
        assert_eq!(host.events(), ["spawn", "handshake dropped", "kill"]);
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
    async fn a_widget_that_exits_during_the_handshake_stops_the_listening() {
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
            ["spawn", "handshake dropped"],
            "nothing left to kill, and nothing left listening"
        );
    }
}

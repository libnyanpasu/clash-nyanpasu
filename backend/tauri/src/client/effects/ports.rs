//! Effect dispatch, commit notifications and the capabilities consumed by the effects executor.
//!
//! The facade holds a single dispatch dependency and the
//! fan-out to actor clients and adapters stays inside the executor. There is no
//! lookup API here, so this is not a service locator.

use nyanpasu_config::{
    application::{
        I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NetworkStatisticWidgetLogoSource,
    },
    runtime::executor::ResolvedPortBindings,
};
use nyanpasu_widget_protocol::ipc::WidgetIpcError;
use snafu::Snafu;

#[cfg(test)]
use super::status::EffectHealth;
use super::{
    plan::{ApplicationEffectPlan, TrayView, WidgetLogoState},
    status::{EffectFailureCode, EffectRevision, EffectStatus},
};

/// The process-wide i18n locale that the tray menu labels are rendered from.
/// Setting it cannot fail.
#[cfg_attr(test, mockall::automock)]
pub trait LocaleSink: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage);
}

/// Why the tray could not be asked to refresh. Whatever goes wrong once the
/// work is queued is logged where the tray runs it.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum TrayError {
    #[snafu(display("could not schedule the tray work on the main thread"))]
    ScheduleTrayWork {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl TrayError {
    pub fn code(&self) -> EffectFailureCode {
        EffectFailureCode::TrayRefreshFailed
    }
}

/// Why the running logger could not be reconfigured.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LoggerError {
    #[snafu(display("the logger reload thread has stopped"))]
    ReloadThreadStopped,
}

impl LoggerError {
    pub fn code(&self) -> EffectFailureCode {
        EffectFailureCode::LoggerRefreshFailed
    }
}

/// A tray rebuild (`refresh_full`) or a refresh of the parts that change with
/// state (`refresh_part`), both rendered from `view`. Async because the
/// concrete implementation has to reach the main thread to touch the tray at
/// all.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait TrayRefresher: Send + Sync + 'static {
    async fn refresh_full(&self, view: TrayView) -> Result<(), TrayError>;
    async fn refresh_part(&self, view: TrayView) -> Result<(), TrayError>;
}

/// How the log file is split and how many of its files are kept. The two
/// limits change together because the file writer is rebuilt from both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRotation {
    pub max_files: usize,
    /// Split the current file once it exceeds this many MiB.
    pub max_file_size: u64,
}

/// Reconfigures the running logger.
///
/// The two `Option`s are the shape the underlying reload signal already has:
/// `None` means "leave this half alone". The plan always carries both, but the
/// port keeps the signal's own vocabulary so the adapter stays a pass-through.
#[cfg_attr(test, mockall::automock)]
pub trait LoggerRefresher: Send + Sync + 'static {
    fn refresh(
        &self,
        level: Option<LoggingLevel>,
        rotation: Option<LogRotation>,
    ) -> Result<(), LoggerError>;
}

/// Why a widget effect could not be applied.
///
/// A controller that has not been handed its runtime yet is a startup
/// ordering fact the caller can retry, while every other variant names a step
/// of starting or stopping the widget itself. `StillOwned` is a stop that
/// ran out of time: the widget is still owned, and the caller must not report
/// it gone.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum WidgetError {
    #[snafu(display("the network statistic widget is not available yet"))]
    Unavailable,
    #[snafu(display("the app is shutting down; no widget starts"))]
    ShuttingDown,
    #[snafu(display("could not stop the running widget before starting another"))]
    StopPrevious {
        #[snafu(source(from(WidgetError, Box::new)))]
        source: Box<WidgetError>,
    },
    #[snafu(display("could not locate the running executable"))]
    LocateExecutable { source: std::io::Error },
    #[snafu(display("could not create the widget's IPC server"))]
    CreateIpcServer { source: WidgetIpcError },
    #[snafu(display("could not hand the app's output to the widget"))]
    DuplicateStdio { source: std::io::Error },
    #[snafu(display("could not spawn the {variant} widget"))]
    SpawnWidget {
        variant: String,
        source: std::io::Error,
    },
    #[snafu(display("the widget did not connect back"))]
    ConnectWidget { source: WidgetIpcError },
    #[snafu(display("the widget process exited before it connected: {status}"))]
    WidgetExited { status: String },
    #[snafu(display("could not wait for the widget process"))]
    WaitWidget { source: std::io::Error },
    #[snafu(display("the app is shutting down before the widget connected"))]
    ShutdownBeforeConnect,
    #[snafu(display("widget process still owned, exit not confirmed"))]
    StillOwned,
}

impl WidgetError {
    pub fn code(&self) -> EffectFailureCode {
        match self {
            Self::Unavailable => EffectFailureCode::WidgetUnavailable,
            _ => EffectFailureCode::WidgetApplyFailed,
        }
    }
}

/// Drives the network statistic widget towards a desired configuration.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetController: Send + Sync + 'static {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError>;
    /// Shows the icon of `state` from `source`: in the running widget now,
    /// and in every widget started later.
    async fn show_logo(
        &self,
        source: NetworkStatisticWidgetLogoSource,
        state: WidgetLogoState,
    ) -> Result<(), WidgetError>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ApplicationEffectsPort: Send + Sync + 'static {
    /// Applies the plan in its own order and reports one status per effect.
    ///
    /// Infallible by design: the configuration is already committed, so a
    /// failure has to surface as health rather than as an error that would
    /// suggest nothing was written. Each owner compares `revision` against what
    /// it last applied and reports `Superseded` for anything older.
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus>;
}

/// Accepts every plan and changes nothing. The composition root now assembles
/// a real executor, so this is only for tests that care about the pipeline
/// rather than about any particular effect.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct NoopApplicationEffects;

#[cfg(test)]
#[async_trait::async_trait]
impl ApplicationEffectsPort for NoopApplicationEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        plan.effects()
            .iter()
            .map(|effect| EffectStatus {
                kind: effect.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: EffectHealth::Healthy,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::effects::plan::ApplicationEffectInputs;
    use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};

    #[tokio::test]
    async fn noop_port_reports_healthy_for_every_effect() {
        let inputs = ApplicationEffectInputs::project(
            &NyanpasuAppConfig::default(),
            &ClashConfig::default(),
            None,
        );
        let plan = ApplicationEffectPlan::full(&inputs);
        let revision = EffectRevision::new(3);

        let statuses = NoopApplicationEffects.apply(revision, plan.clone()).await;

        assert_eq!(statuses.len(), plan.effects().len());
        for (status, effect) in statuses.iter().zip(plan.effects()) {
            assert_eq!(status.kind, effect.kind());
            assert_eq!(status.desired_revision, revision);
            assert_eq!(status.applied_revision, revision);
            assert_eq!(status.health, EffectHealth::Healthy);
        }
    }
}

/// Post-commit notification only: never a source-transaction vote.
///
/// One method per slice of [`ApplicationEffectInputs`], and each slice has
/// exactly one sender: the serial owner of that domain. Nobody reads or
/// forwards a sibling's slice; the effects owner combines them.
///
/// [`ApplicationEffectInputs`]: super::plan::ApplicationEffectInputs
pub(crate) trait CommitNotifications: Send + Sync + 'static {
    /// The application owner, once its commit has settled. `requested` names
    /// the owners the request asked for even when their inputs did not move.
    fn application_committed(
        &self,
        fields: super::plan::ApplicationEffectFields,
        requested: Vec<super::plan::EffectKind>,
    );

    /// The clash config owner, once its commit has settled.
    fn clash_committed(&self, fields: super::plan::ClashEffectFields);

    /// The profiles owner. No effect reads the profiles, so a commit only asks
    /// the tray for a partial refresh. `files` names the profiles whose file
    /// content the commit may have changed or removed.
    fn profiles_committed(&self, files: Vec<nyanpasu_config::profile::ProfileId>);

    /// The Runtime, after a mutation's Confirm or Cancel, a lifecycle command
    /// or a recovery: the ports the core is bound to now. `refresh` asks the
    /// tray for a partial refresh even when nothing it reads moved.
    fn runtime_bound(&self, ports: Option<ResolvedPortBindings>, refresh: bool);

    /// Hands every owner its complete desired value and rebuilds the tray.
    /// StartupReconcile sends it once (T10 §1.9), with the ports it bound.
    fn publish_full(&self, ports: Option<ResolvedPortBindings>);
}

/// Tells both receivers about every commit, the first one first.
impl<A: CommitNotifications, B: CommitNotifications> CommitNotifications for (A, B) {
    fn application_committed(
        &self,
        fields: super::plan::ApplicationEffectFields,
        requested: Vec<super::plan::EffectKind>,
    ) {
        self.0
            .application_committed(fields.clone(), requested.clone());
        self.1.application_committed(fields, requested);
    }

    fn clash_committed(&self, fields: super::plan::ClashEffectFields) {
        self.0.clash_committed(fields.clone());
        self.1.clash_committed(fields);
    }

    fn profiles_committed(&self, files: Vec<nyanpasu_config::profile::ProfileId>) {
        self.0.profiles_committed(files.clone());
        self.1.profiles_committed(files);
    }

    fn runtime_bound(&self, ports: Option<ResolvedPortBindings>, refresh: bool) {
        self.0.runtime_bound(ports.clone(), refresh);
        self.1.runtime_bound(ports, refresh);
    }

    fn publish_full(&self, ports: Option<ResolvedPortBindings>) {
        self.0.publish_full(ports.clone());
        self.1.publish_full(ports);
    }
}

#[cfg(test)]
pub(crate) struct NoopCommitNotifications;
#[cfg(test)]
impl CommitNotifications for NoopCommitNotifications {
    fn application_committed(
        &self,
        _: super::plan::ApplicationEffectFields,
        _: Vec<super::plan::EffectKind>,
    ) {
    }

    fn clash_committed(&self, _: super::plan::ClashEffectFields) {}

    fn profiles_committed(&self, _: Vec<nyanpasu_config::profile::ProfileId>) {}

    fn runtime_bound(&self, _: Option<ResolvedPortBindings>, _: bool) {}

    fn publish_full(&self, _: Option<ResolvedPortBindings>) {}
}

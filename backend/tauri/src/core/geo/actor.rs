//! Owns the country index and keeps it in step with the core.
use std::{sync::Arc, time::Duration};

use nyanpasu_geodata::IpIndex;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::{sync::watch, task::JoinHandle};

use super::ports::{CountryIndexSource, GeodataMode, IndexKey, Loaded, OnChange};
use crate::core::actor_v2::CoreClient;

/// How long to wait before asking a core that did not answer again.
const RETRY: Duration = Duration::from_secs(1);

pub struct GeoIndexArgs {
    pub source: Arc<dyn CountryIndexSource>,
    pub core: CoreClient,
}

pub(super) enum Message {
    /// A core instance answered `GET /configs`.
    Mode(GeodataMode),
    /// A country database in the core's home was written.
    FilesChanged,
}

pub(super) struct GeoIndexActor;

pub(super) struct State {
    source: Arc<dyn CountryIndexSource>,
    mode: Option<GeodataMode>,
    key: Option<IndexKey>,
    published: watch::Sender<Option<Arc<IpIndex>>>,
    /// Dropping it ends the watch.
    _watch: Option<Box<dyn Send>>,
    follower: JoinHandle<()>,
}

impl Actor for GeoIndexActor {
    type Msg = Message;
    type State = State;
    type Arguments = (GeoIndexArgs, watch::Sender<Option<Arc<IpIndex>>>);

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        (args, published): Self::Arguments,
    ) -> Result<State, ActorProcessingErr> {
        let GeoIndexArgs { source, core } = args;
        let changed = {
            let myself = myself.clone();
            OnChange::new(move || {
                let _ = myself.cast(Message::FilesChanged);
            })
        };
        // Without the watch the index still follows every new core instance.
        let watch = match source.watch(changed) {
            Ok(guard) => Some(guard),
            Err(error) => {
                tracing::warn!(
                    "geo database updates are not watched: {}",
                    snafu::Report::from_error(error)
                );
                None
            }
        };
        Ok(State {
            source,
            mode: None,
            key: None,
            published,
            _watch: watch,
            follower: tokio::spawn(follow_core(myself, core)),
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Message::Mode(mode) = message {
            state.mode = Some(mode);
        }
        state.reload().await;
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.follower.abort();
        if let Err(error) = (&mut state.follower).await
            && let Ok(panic) = error.try_into_panic()
        {
            std::panic::resume_unwind(panic);
        }
        Ok(())
    }
}

impl State {
    async fn reload(&mut self) {
        // Nothing loads before the core reports which database it reads.
        let Some(mode) = self.mode else {
            return;
        };
        let (source, current) = (self.source.clone(), self.key.clone());
        let loaded = nyanpasu_core::tasks::blocking::join(
            tokio::task::spawn_blocking(move || source.load(mode, current)).await,
        );
        match loaded {
            Ok(Loaded::Unchanged) => {}
            Ok(Loaded::Missing) => {
                self.key = None;
                self.published.send_replace(None);
            }
            Ok(Loaded::Index { key, index }) => {
                self.key = Some(key);
                self.published.send_replace(Some(index));
            }
            // The previous index describes an older database, which beats none.
            Err(error) => tracing::warn!(
                "the country index was not reloaded: {}",
                snafu::Report::from_error(error)
            ),
        }
    }
}

/// Reads `geodata-mode` from every core instance as it binds. Cores without the field (clash-rs,
/// Clash Premium) read the mmdb.
async fn follow_core(actor: ActorRef<Message>, core: CoreClient) {
    loop {
        let Ok(api) = core.api_client().await else {
            tokio::time::sleep(RETRY).await;
            continue;
        };
        loop {
            match api.configs().await {
                Ok(config) => {
                    let mode = match config.geodata_mode {
                        Some(true) => GeodataMode::Dat,
                        _ => GeodataMode::Mmdb,
                    };
                    if actor.cast(Message::Mode(mode)).is_err() {
                        return;
                    }
                    break;
                }
                Err(_) if api.is_revoked() => break,
                Err(error) => {
                    tracing::debug!(%error, "the core did not report its geodata mode");
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
        api.cancelled().await;
    }
}

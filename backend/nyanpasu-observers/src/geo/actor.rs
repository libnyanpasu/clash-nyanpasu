//! Owns the country index and keeps it in step with the core.
use std::sync::Arc;

use nyanpasu_geodata::IpIndex;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::{sync::watch, task::JoinHandle};

use super::ports::{
    CountryIndexSource, GeodataMode, IndexKey, Loaded, ModeReporter, ModeSource, OnChange,
};

pub struct GeoIndexArgs {
    pub source: Arc<dyn CountryIndexSource>,
    pub mode_source: Arc<dyn ModeSource>,
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
        let GeoIndexArgs {
            source,
            mode_source,
        } = args;
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
            follower: mode_source.start(ModeReporter::new(myself)),
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
        let loaded = nyanpasu_owner_support::blocking::join(
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

//! Per-uid background refresh/watch services owned by ProfilesActor.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Context as _;
use notify_debouncer_full::{
    DebounceEventResult, Debouncer, RecommendedCache, new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
};
use nyanpasu_config::profile::{
    ExternalMode, ExternalProfilePath, LocalBinding, ProfileId, ProfileSource, Profiles,
};
use ractor::ActorRef;

use super::actor::ProfilesActorMessage;

struct WatchEntry {
    watch_path: PathBuf,
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

#[derive(Default)]
pub(super) struct ExternalWatchers {
    entries: HashMap<ProfileId, WatchEntry>,
}

impl ExternalWatchers {
    pub(super) fn reconcile(
        &mut self,
        profiles: &Profiles,
        actor: &ActorRef<ProfilesActorMessage>,
    ) {
        let desired: HashMap<ProfileId, PathBuf> = profiles
            .items
            .iter()
            .filter_map(|(uid, item)| match item.definition.source() {
                Some(ProfileSource::Local {
                    binding: LocalBinding::External { target, mode, .. },
                }) => Some((uid.clone(), watch_path(target, *mode))),
                _ => None,
            })
            .collect();

        let stale: Vec<ProfileId> = self
            .entries
            .iter()
            .filter(|(uid, entry)| desired.get(*uid) != Some(&entry.watch_path))
            .map(|(uid, _)| uid.clone())
            .collect();
        for uid in stale {
            self.entries.remove(&uid);
        }

        for (uid, watch_path) in desired {
            if self.entries.contains_key(&uid) {
                continue;
            }

            let actor = actor.clone();
            let event_uid = uid.clone();
            let mut debouncer = match new_debouncer(
                std::time::Duration::from_millis(500),
                None,
                move |events: DebounceEventResult| match events {
                    Ok(events) => {
                        if !events.is_empty() {
                            let _ = actor.cast(ProfilesActorMessage::ExternalFileChanged {
                                uid: event_uid.clone(),
                            });
                        }
                    }
                    Err(errors) => {
                        tracing::warn!(?errors, "failed to debounce external profile event");
                    }
                },
            ) {
                Ok(debouncer) => debouncer,
                Err(error) => {
                    tracing::warn!(
                        uid = %uid,
                        path = %watch_path.display(),
                        error = %error,
                        "failed to create external profile watcher"
                    );
                    continue;
                }
            };

            if let Err(error) = debouncer
                .watch(&watch_path, RecursiveMode::NonRecursive)
                .with_context(|| format!("watch external profile {}", watch_path.display()))
            {
                tracing::warn!(
                    uid = %uid,
                    path = %watch_path.display(),
                    error = %error,
                    "failed to watch external profile target"
                );
                continue;
            }

            self.entries.insert(
                uid,
                WatchEntry {
                    watch_path,
                    _debouncer: debouncer,
                },
            );
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.entries.clear();
    }
}

fn watch_path(target: &ExternalProfilePath, mode: ExternalMode) -> PathBuf {
    match mode {
        ExternalMode::Symlink => std::fs::canonicalize(target.as_path())
            .unwrap_or_else(|_| target.as_path().to_path_buf()),
        ExternalMode::Mirror => target.as_path().to_path_buf(),
    }
}

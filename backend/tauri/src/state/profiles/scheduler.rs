//! Per-uid remote refresh timers owned by ProfilesActor.

use std::collections::HashMap;

use nyanpasu_config::profile::{ProfileId, ProfileSource, Profiles};
use ractor::ActorRef;
use tokio::task::JoinHandle;

use super::actor::ProfilesActorMessage;

struct Entry {
    interval_minutes: u64,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct RemoteUpdateScheduler {
    entries: HashMap<ProfileId, Entry>,
}

impl RemoteUpdateScheduler {
    pub(super) fn reconcile(
        &mut self,
        profiles: &Profiles,
        actor: &ActorRef<ProfilesActorMessage>,
        catch_up: bool,
    ) {
        let desired: HashMap<ProfileId, (u64, Option<time::OffsetDateTime>)> = profiles
            .items
            .iter()
            .filter_map(|(uid, item)| match item.definition.source() {
                Some(ProfileSource::Remote {
                    option,
                    materialized,
                    ..
                }) if option.update_interval_minutes > 0 => Some((
                    uid.clone(),
                    (option.update_interval_minutes, materialized.updated_at),
                )),
                _ => None,
            })
            .collect();

        let stale: Vec<ProfileId> = self
            .entries
            .iter()
            .filter(|(uid, entry)| {
                desired
                    .get(*uid)
                    .is_none_or(|(interval, _)| *interval != entry.interval_minutes)
            })
            .map(|(uid, _)| uid.clone())
            .collect();
        for uid in stale {
            if let Some(entry) = self.entries.remove(&uid) {
                entry.handle.abort();
            }
        }

        for (uid, (interval_minutes, updated_at)) in desired {
            if self.entries.contains_key(&uid) {
                continue;
            }
            let overdue = catch_up
                && updated_at.is_none_or(|at| {
                    time::OffsetDateTime::now_utc() - at
                        >= time::Duration::minutes(interval_minutes as i64)
                });
            let actor = actor.clone();
            let tick_uid = uid.clone();
            let handle = tokio::spawn(async move {
                if overdue {
                    let _ = actor.cast(ProfilesActorMessage::RefreshRemote {
                        uid: tick_uid.clone(),
                        patch: None,
                        reply: None,
                    });
                }
                let period = std::time::Duration::from_secs(interval_minutes * 60);
                loop {
                    tokio::time::sleep(period).await;
                    let _ = actor.cast(ProfilesActorMessage::RefreshRemote {
                        uid: tick_uid.clone(),
                        patch: None,
                        reply: None,
                    });
                }
            });
            self.entries.insert(
                uid,
                Entry {
                    interval_minutes,
                    handle,
                },
            );
        }
    }

    pub(super) fn shutdown(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.handle.abort();
        }
    }
}

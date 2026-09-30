use futures_util::StreamExt;
use nyanpasu_traffic::{
    accounting::{self, AccountingResult},
    model::*,
    ports::*,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;

pub struct TrafficActorArgs {
    pub host: HostId,
    pub source: Arc<dyn TrafficSource>,
    pub store: Arc<dyn TrafficStore>,
    pub clock: Arc<dyn Clock>,
    pub cancellation: CancellationToken,
}
pub(crate) struct StartArgs {
    pub args: TrafficActorArgs,
    pub summary: watch::Sender<Option<TrafficSummary>>,
    pub details: watch::Sender<Option<TrafficDetails>>,
    pub stopped: watch::Sender<Option<TrafficResult<()>>>,
    pub status: watch::Sender<Option<StoreError>>,
}
pub(crate) enum Message {
    Select(Option<String>, Option<RpcReplyPort<TrafficResult<()>>>),
    Start(
        NewSession,
        Option<SourceBinding>,
        Option<RpcReplyPort<TrafficResult<SessionRecord>>>,
    ),
    Bind(
        SourceBinding,
        Option<Arc<dyn TrafficSource>>,
        Option<RpcReplyPort<TrafficResult<()>>>,
    ),
    Detach(String, Option<RpcReplyPort<TrafficResult<()>>>),
    Exit(
        String,
        UInt,
        Option<RpcReplyPort<TrafficResult<CommitReceipt>>>,
    ),
    Context(
        String,
        Option<ConfigContext>,
        RpcReplyPort<TrafficResult<()>>,
    ),
    Observe(Observation, RpcReplyPort<TrafficResult<CommitReceipt>>),
    Disconnected(
        String,
        UInt,
        Option<StoreError>,
        RpcReplyPort<TrafficResult<()>>,
    ),
    CurrentSession(RpcReplyPort<TrafficResult<Option<SessionRecord>>>),
    Session(SessionId, RpcReplyPort<TrafficResult<SessionRecord>>),
    Connections(
        ConnectionsQuery,
        RpcReplyPort<TrafficResult<ConnectionPage>>,
    ),
    Usage(UsageQuery, RpcReplyPort<TrafficResult<UsageResult>>),
    Topology(TopologyQuery, RpcReplyPort<TrafficResult<TopologyResult>>),
    #[cfg(test)]
    ResidentActive(RpcReplyPort<TrafficResult<usize>>),
}
pub(crate) struct TrafficActor;
struct Instance {
    session: SessionRecord,
    active: BTreeMap<String, ConnectionRecord>,
    active_loaded: bool,
    rates: BTreeMap<String, Option<Rate>>,
    rate: Option<Rate>,
    context: Option<ConfigContext>,
    continuous: bool,
    interval_valid: bool,
    generation: UInt,
    pending_end: Option<UInt>,
    error: Option<StoreError>,
    pending: Option<AccountingResult>,
    worker: Option<(CancellationToken, JoinHandle<()>)>,
}
pub(crate) struct State {
    args: TrafficActorArgs,
    instances: BTreeMap<String, Instance>,
    current: Option<String>,
    pending_context: Option<(String, Option<ConfigContext>)>,
    pending_start: Option<(NewSession, Option<SourceBinding>, Option<UInt>)>,
    pending_start_error: Option<StoreError>,
    lost_lifecycle_error: Option<StoreError>,
    summary: watch::Sender<Option<TrafficSummary>>,
    details: watch::Sender<Option<TrafficDetails>>,
    stopped: watch::Sender<Option<TrafficResult<()>>>,
    status: watch::Sender<Option<StoreError>>,
}
fn reply<T>(port: RpcReplyPort<T>, value: T) {
    let _ = port.send(value);
}
fn member_rates(instance: &Instance) -> BTreeMap<String, Option<Rate>> {
    let mut rates: BTreeMap<String, Option<Rate>> = BTreeMap::new();
    for (id, record) in &instance.active {
        let rate = if instance.session.freshness == Freshness::Fresh {
            instance.rates.get(id).cloned().flatten()
        } else {
            None
        };
        for name in record
            .dimensions
            .path
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let total = rates.entry(name.clone()).or_insert(Some(Rate {
                upload: 0.0,
                download: 0.0,
            }));
            match (total.as_mut(), rate.as_ref()) {
                (Some(total), Some(rate)) => {
                    total.upload += rate.upload;
                    total.download += rate.download
                }
                _ => *total = None,
            }
        }
    }
    rates
}
impl State {
    fn publish(&self, id: &str) {
        if self.current.as_deref() != Some(id) {
            return;
        }
        if let Some(i) = self.instances.get(id) {
            if !i.active_loaded {
                self.summary.send_replace(None);
                self.details.send_replace(None);
                return;
            }
            self.summary.send_replace(Some(TrafficSummary {
                session: i.session.clone(),
                revision: i.session.position.sequence,
                current_rate: i.rate.clone(),
                active_connections: UInt(i.active.len() as u64),
                member_rates: member_rates(i),
                discrepancy: i.session.discrepancy(),
            }));
            if self.details.receiver_count() > 0 {
                self.details.send_replace(Some(TrafficDetails {
                    freshness: i.session.freshness.clone(),
                    session_id: i.session.id.clone(),
                    revision: i.session.position.sequence,
                    connections: i.active.values().cloned().collect(),
                    rates: i.rates.clone(),
                }));
            } else {
                self.details.send_replace(None);
            }
        }
    }
    fn refresh_status(&self) {
        let error = self
            .pending_start_error
            .clone()
            .or_else(|| self.lost_lifecycle_error.clone())
            .or_else(|| {
                self.current
                    .as_ref()
                    .and_then(|id| self.instances.get(id))
                    .and_then(|i| i.error.clone())
            })
            .or_else(|| self.instances.values().find_map(|i| i.error.clone()));
        self.status.send_replace(error);
    }
    fn lifecycle_failure(&mut self, id: &str, error: &StoreError) {
        if let Some(i) = self.instances.get_mut(id) {
            i.error = Some(error.clone());
            i.continuous = false;
            i.interval_valid = false;
            i.rate = None;
            i.rates.clear();
            i.session.freshness = Freshness::Unavailable;
            accounting::add_quality(&mut i.session.quality, Quality::StorageDegraded);
            accounting::add_quality(&mut i.session.quality, Quality::LifecycleUnknown);
            self.publish(id);
        } else {
            self.pending_start_error = Some(error.clone());
        }
        self.refresh_status();
    }
    async fn retry_lifecycle(&mut self, actor: ActorRef<Message>) {
        if let Some((new, binding, end)) = self.pending_start.take() {
            let id = new.instance_id.clone();
            let result = self.start(new.clone(), binding.clone(), actor).await;
            match result {
                Ok(_) => {
                    self.pending_start_error = None;
                    if let Some(time) = end
                        && let Err(error) = self.exit(id.clone(), time).await
                    {
                        self.lifecycle_failure(&id, &error);
                    }
                }
                Err(error) => {
                    self.pending_start = Some((new, binding, end));
                    self.lifecycle_failure(&id, &error);
                }
            }
        }
        // Each failed exit remains owned by its instance until a later command resolves it.
        let pending: Vec<_> = self
            .instances
            .iter()
            .filter_map(|(id, i)| i.pending_end.map(|time| (id.clone(), time)))
            .collect();
        for (id, time) in pending {
            if let Err(error) = self.exit(id.clone(), time).await {
                self.lifecycle_failure(&id, &error);
            }
        }
        self.refresh_status();
    }
    fn overlay(&self, session: &mut SessionRecord) {
        if let Some(i) = self.instances.values().find(|i| i.session.id == session.id) {
            session.freshness = i.session.freshness.clone();
            for flag in &i.session.quality {
                accounting::add_quality(&mut session.quality, flag.clone());
            }
        }
    }
    fn matching_rates(
        &self,
        id: &SessionId,
        filter: &ConnectionFilter,
    ) -> (Option<Rate>, BTreeMap<String, Option<Rate>>) {
        let Some(i) = self.instances.values().find(|i| &i.session.id == id) else {
            return (None, BTreeMap::new());
        };
        if i.session.freshness != Freshness::Fresh || !i.interval_valid {
            return (None, BTreeMap::new());
        }
        let mut sum = Rate {
            upload: 0.0,
            download: 0.0,
        };
        let mut rates = BTreeMap::new();
        let mut known = true;
        for (id, record) in &i.active {
            if accounting::matches_dimensions(&record.dimensions, filter) {
                let rate = i.rates.get(id).cloned().flatten();
                if let Some(rate) = &rate {
                    sum.upload += rate.upload;
                    sum.download += rate.download
                } else {
                    known = false
                }
                rates.insert(id.clone(), rate);
            }
        }
        (if known { Some(sum) } else { None }, rates)
    }
    async fn usage(&self, q: UsageQuery) -> TrafficResult<UsageResult> {
        let mut result = self.args.store.query_usage(q.clone()).await?;
        self.overlay(&mut result.meta.session);
        if matches!(q.scope, QueryScope::MinuteWindow { .. }) {
            return Ok(result);
        }
        let (rate, _) = self.matching_rates(&q.session_id, &q.filter);
        result.current_rate = rate;
        if let Some(group) = q.group_by
            && let Some(i) = self
                .instances
                .values()
                .find(|i| i.session.id == q.session_id)
        {
            for row in &mut result.groups {
                let mut sum = Rate {
                    upload: 0.0,
                    download: 0.0,
                };
                let mut known = i.session.freshness == Freshness::Fresh && i.interval_valid;
                for (id, r) in &i.active {
                    if accounting::matches_dimensions(&r.dimensions, &q.filter)
                        && accounting::group_key(&r.dimensions, &group) == row.key
                    {
                        if let Some(rate) = i.rates.get(id).and_then(|r| r.as_ref()) {
                            sum.upload += rate.upload;
                            sum.download += rate.download
                        } else {
                            known = false
                        }
                    }
                }
                row.current_rate = if known { Some(sum) } else { None };
            }
        }
        Ok(result)
    }
    async fn topology(&self, q: TopologyQuery) -> TrafficResult<TopologyResult> {
        let mut result = self.args.store.query_topology(q.clone()).await?;
        self.overlay(&mut result.meta.session);
        if !matches!(q.scope, QueryScope::MinuteWindow { .. }) {
            for path in &mut result.paths {
                let Some(i) = self
                    .instances
                    .values()
                    .find(|i| i.session.id == q.session_id)
                else {
                    continue;
                };
                if i.session.freshness != Freshness::Fresh || !i.interval_valid {
                    continue;
                }
                let mut total = Rate {
                    upload: 0.0,
                    download: 0.0,
                };
                let mut known = true;
                for (id, r) in &i.active {
                    if nyanpasu_traffic::topology::path_dimensions(&r.dimensions) == path.dimensions
                        && accounting::matches_dimensions(&r.dimensions, &q.filter)
                    {
                        if let Some(rate) = i.rates.get(id).and_then(|r| r.as_ref()) {
                            total.upload += rate.upload;
                            total.download += rate.download
                        } else {
                            known = false
                        }
                    }
                }
                path.current_rate = if known { Some(total) } else { None };
            }
        }
        let (nodes, edges) = nyanpasu_traffic::topology::project(result.paths.clone())?;
        result.nodes = nodes;
        result.edges = edges;
        Ok(result)
    }
    async fn stop_worker(i: &mut Instance) {
        if let Some((cancel, join)) = i.worker.take() {
            cancel.cancel();
            if let Err(error) = join.await
                && error.is_panic()
            {
                std::panic::resume_unwind(error.into_panic())
            }
        }
    }
    async fn load_active(store: &dyn TrafficStore, i: &mut Instance) -> TrafficResult<()> {
        if i.active_loaded || i.session.ended_at.is_some() {
            return Ok(());
        }
        let mut active = BTreeMap::new();
        let mut cursor = None;
        loop {
            let page = store
                .query_connections(ConnectionsQuery {
                    session_id: i.session.id.clone(),
                    filter: ConnectionFilter {
                        status: Some(true),
                        ..Default::default()
                    },
                    limit: 500,
                    cursor,
                })
                .await?;
            active.extend(
                page.connections
                    .into_iter()
                    .map(|record| (record.id.clone(), record)),
            );
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        i.active = active;
        i.active_loaded = true;
        Ok(())
    }
    async fn start(
        &mut self,
        new: NewSession,
        binding: Option<SourceBinding>,
        actor: ActorRef<Message>,
    ) -> TrafficResult<SessionRecord> {
        if new.host != self.args.host {
            return Err(StoreError::InvalidData(
                "session host differs from traffic owner".into(),
            ));
        }
        let session = self.args.store.begin_session(new).await?;
        let id = session.instance_id.clone();
        self.instances
            .entry(id.clone())
            .or_insert_with(|| Instance {
                session: session.clone(),
                active: BTreeMap::new(),
                active_loaded: session.observed_connections.0 == 0,
                rates: BTreeMap::new(),
                rate: None,
                context: None,
                continuous: false,
                interval_valid: false,
                generation: session.source_generation,
                pending_end: None,
                error: None,
                pending: None,
                worker: None,
            });

        let instance = self.instances.get_mut(&id).expect("inserted instance");
        Self::load_active(self.args.store.as_ref(), instance).await?;
        if instance
            .session
            .quality
            .contains(&Quality::LifecycleUnknown)
            && instance.pending.is_none()
            && instance.pending_end.is_none()
        {
            instance.error = None;
        }
        instance
            .session
            .quality
            .retain(|q| *q != Quality::LifecycleUnknown);
        self.refresh_status();
        if self
            .pending_context
            .as_ref()
            .is_some_and(|(instance, _)| instance == &id)
            && let Some((_, context)) = self.pending_context.take()
        {
            self.instances
                .get_mut(&id)
                .expect("inserted instance")
                .context = context;
        }
        if let Some(binding) = binding {
            self.bind(binding, None, actor).await?
        }
        self.publish(&id);
        let pruned = self
            .args
            .store
            .prune(RetentionPolicy {
                host: self.args.host.clone(),
                keep_last_ended: true,
                protected_session: self
                    .current
                    .as_ref()
                    .and_then(|id| self.instances.get(id))
                    .map(|i| i.session.id.clone()),
            })
            .await?;
        self.instances
            .retain(|_, instance| !pruned.removed.contains(&instance.session.id));
        Ok(self
            .instances
            .get(&id)
            .expect("retained active instance")
            .session
            .clone())
    }
    async fn detach(&mut self, id: &str) -> TrafficResult<()> {
        // A lifecycle retry may persist the session later, but must not resume a detached source.
        let pending_detached = if let Some((new, binding, _)) = &mut self.pending_start
            && new.instance_id == id
        {
            *binding = None;
            true
        } else {
            false
        };
        let Some(i) = self.instances.get_mut(id) else {
            return if pending_detached {
                Ok(())
            } else {
                Err(StoreError::NotFound)
            };
        };
        Self::stop_worker(i).await;
        i.generation = UInt(
            i.generation
                .0
                .checked_add(1)
                .ok_or_else(|| StoreError::InvalidData("generation overflow".into()))?,
        );
        i.continuous = false;
        i.interval_valid = false;
        i.rate = None;
        i.rates.clear();
        if let Some(pending) = i.pending.take() {
            match self
                .args
                .store
                .commit_observation(pending.commit.clone())
                .await
            {
                Ok(_) => {
                    i.session = pending.commit.session;
                    i.active = pending.active;
                }
                Err(error) => {
                    i.pending = Some(pending);
                    i.error = Some(error.clone());
                    i.session.freshness = Freshness::Unavailable;
                    self.refresh_status();
                    self.publish(id);
                    return Err(error);
                }
            }
        }
        i.active.clear();
        i.active_loaded = false;
        if i.pending_end.is_none() {
            i.error = None;
        }
        if i.session.ended_at.is_none() {
            i.session.freshness = Freshness::Stale;
            accounting::add_quality(&mut i.session.quality, Quality::Gap);
            accounting::add_quality(&mut i.session.quality, Quality::LifecycleUnknown);
        }
        self.refresh_status();
        self.publish(id);
        Ok(())
    }
    async fn bind(
        &mut self,
        binding: SourceBinding,
        source: Option<Arc<dyn TrafficSource>>,
        actor: ActorRef<Message>,
    ) -> TrafficResult<()> {
        let id = binding.instance_id.clone();
        let i = self.instances.get_mut(&id).ok_or(StoreError::NotFound)?;
        if i.session.ended_at.is_some() || i.pending_end.is_some() {
            return Err(StoreError::Conflict(
                "cannot bind an exited instance".into(),
            ));
        }
        Self::load_active(self.args.store.as_ref(), i).await?;
        Self::stop_worker(i).await;
        i.generation = UInt(
            i.generation
                .0
                .checked_add(1)
                .ok_or(StoreError::InvalidData("generation overflow".into()))?,
        );
        let generation = i.generation;
        i.continuous = false;
        i.interval_valid = false;
        i.rate = None;
        i.rates.clear();
        i.session.freshness = Freshness::Stale;
        if i.session.first_sample_at.is_some() {
            accounting::add_quality(&mut i.session.quality, Quality::Gap);
        }
        let cancel = self.args.cancellation.child_token();
        let source = source.unwrap_or_else(|| self.args.source.clone());
        let clock = self.args.clock.clone();
        let worker_cancel = cancel.clone();
        let worker_id = id.clone();
        let join = tokio::spawn(async move {
            loop {
                let connection = tokio::select! {_ = worker_cancel.cancelled()=>break,result=source.connect(binding.clone(),generation,clock.clone())=>result};
                let mut reason = Some(StoreError::Unavailable("connections stream ended".into()));
                match connection {
                    Ok(mut stream) => loop {
                        let frame = tokio::select! {_=worker_cancel.cancelled()=>return,frame=stream.next()=>frame};
                        match frame {
                            Some(Ok(observation)) => {
                                let ack = tokio::select! {_=worker_cancel.cancelled()=>return,result=super::client::call(&actor,|port|Message::Observe(observation,port))=>result};
                                if matches!(ack, Err(StoreError::Cancelled)) {
                                    return;
                                }
                            }
                            Some(Err(error)) => {
                                reason = Some(error);
                                break;
                            }
                            None => break,
                        }
                    },
                    Err(error) => reason = Some(error),
                }
                let _ = tokio::select! {_=worker_cancel.cancelled()=>return,result=super::client::call(&actor,|port|Message::Disconnected(worker_id.clone(),generation,reason,port))=>result};
                tokio::select! {_=worker_cancel.cancelled()=>return,_=tokio::time::sleep(std::time::Duration::from_secs(1))=>{}}
            }
        });
        i.worker = Some((cancel, join));

        self.publish(&id);
        Ok(())
    }
    async fn observe(&mut self, o: Observation) -> TrafficResult<CommitReceipt> {
        let i = self
            .instances
            .get_mut(&o.instance_id)
            .ok_or(StoreError::NotFound)?;
        if i.session.ended_at.is_some() || i.generation != o.generation {
            return Err(StoreError::Conflict("stale source generation".into()));
        }
        if let Err(error) = Self::load_active(self.args.store.as_ref(), i).await {
            i.error = Some(error.clone());
            i.session.freshness = Freshness::Unavailable;
            accounting::add_quality(&mut i.session.quality, Quality::StorageDegraded);
            self.refresh_status();
            self.publish(&o.instance_id);
            return Err(error);
        }
        if let Some(pending) = i.pending.take() {
            match self
                .args
                .store
                .commit_observation(pending.commit.clone())
                .await
            {
                Ok(receipt) => {
                    i.session = pending.commit.session;
                    i.active = pending.active;
                    i.rates.clear();
                    i.rate = None;
                    i.error = None;
                    i.continuous = false;
                    i.interval_valid = false;
                    accounting::add_quality(&mut i.session.quality, Quality::Gap);
                    let _ = receipt;
                }
                Err(e) => {
                    i.pending = Some(pending);
                    i.error = Some(e.clone());
                    self.refresh_status();
                    self.publish(&o.instance_id);
                    return Err(e);
                }
            }
        }
        let unknown_ids = if i.session.observed_connections.0 == 0 {
            Vec::new()
        } else {
            o.connections
                .iter()
                .filter(|c| !i.active.contains_key(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        let archived = match self
            .args
            .store
            .connections_by_ids(i.session.id.clone(), unknown_ids)
            .await
        {
            Ok(records) => records,
            Err(error) => {
                i.error = Some(error.clone());
                i.continuous = false;
                i.interval_valid = false;
                i.rate = None;
                i.rates.clear();
                i.session.freshness = Freshness::Unavailable;
                accounting::add_quality(&mut i.session.quality, Quality::StorageDegraded);
                self.refresh_status();
                self.publish(&o.instance_id);
                return Err(error);
            }
        };
        let mut prior = std::mem::take(&mut i.active);
        prior.extend(archived);
        let result =
            match accounting::account(&i.session, &prior, &o, i.context.as_ref(), i.continuous) {
                Ok(r) => r,
                Err(e) => {
                    i.error = Some(e.clone());
                    prior.retain(|_, r| matches!(r.status, ConnectionStatus::Active));
                    i.active = prior;
                    i.continuous = false;
                    i.interval_valid = false;
                    i.rate = None;
                    i.rates.clear();
                    i.session.freshness = Freshness::Stale;
                    accounting::add_quality(&mut i.session.quality, Quality::InvalidCounters);
                    self.refresh_status();
                    self.publish(&o.instance_id);
                    return Err(e);
                }
            };
        match self
            .args
            .store
            .commit_observation(result.commit.clone())
            .await
        {
            Ok(receipt) => {
                i.session = result.commit.session;
                i.active = result.active;
                i.rates = result.rates;
                i.rate = result.current_rate;
                i.error = None;
                i.continuous = true;
                i.interval_valid = result.interval_valid;
                self.refresh_status();
                self.publish(&o.instance_id);
                Ok(receipt)
            }
            Err(e) => {
                i.error = Some(e.clone());
                prior.retain(|_, r| matches!(r.status, ConnectionStatus::Active));
                i.active = prior;
                i.continuous = false;
                i.interval_valid = false;
                i.rate = None;
                i.rates.clear();
                i.session.freshness = Freshness::Stale;
                accounting::add_quality(&mut i.session.quality, Quality::StorageDegraded);
                if matches!(e, StoreError::UnknownOutcome(_)) {
                    i.pending = Some(result)
                }
                self.refresh_status();
                self.publish(&o.instance_id);
                Err(e)
            }
        }
    }
    async fn exit(&mut self, id: String, time: UInt) -> TrafficResult<CommitReceipt> {
        let i = self.instances.get_mut(&id).ok_or(StoreError::NotFound)?;
        i.pending_end = Some(time);
        Self::stop_worker(i).await;
        if let Some(p) = i.pending.take() {
            match self.args.store.commit_observation(p.commit.clone()).await {
                Ok(_) => {
                    i.session = p.commit.session;
                    i.active = p.active
                }
                Err(e) => {
                    i.pending = Some(p);
                    return Err(e);
                }
            }
        }

        let receipt = self
            .args
            .store
            .finish_session(SessionEnd {
                session_id: i.session.id.clone(),
                detected_at: time,
                reason: CloseReason::CoreExited,
            })
            .await?;
        i.session = self.args.store.session(i.session.id.clone()).await?;
        i.pending_end = None;
        i.error = None;
        i.active.clear();
        i.rates.clear();
        i.rate = None;
        i.continuous = false;
        i.interval_valid = false;
        self.refresh_status();
        self.publish(&id);
        let protected_session = self
            .current
            .as_ref()
            .and_then(|id| self.instances.get(id))
            .map(|i| i.session.id.clone());
        let pruned = self
            .args
            .store
            .prune(RetentionPolicy {
                host: self.args.host.clone(),
                keep_last_ended: true,
                protected_session,
            })
            .await?;
        self.instances
            .retain(|_, i| !pruned.removed.contains(&i.session.id));
        Ok(receipt)
    }
}
impl Actor for TrafficActor {
    type Msg = Message;
    type State = State;
    type Arguments = StartArgs;
    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: StartArgs,
    ) -> Result<State, ActorProcessingErr> {
        let recovered = args.args.store.recover(args.args.host.clone()).await?;
        let mut instances = BTreeMap::new();
        for recovered in recovered.sessions {
            let mut session = recovered.session;
            if session.ended_at.is_none() {
                session.freshness = Freshness::Stale;
                accounting::add_quality(&mut session.quality, Quality::Gap);
                accounting::add_quality(&mut session.quality, Quality::LifecycleUnknown);
            }
            let generation = session.source_generation;
            instances.insert(
                session.instance_id.clone(),
                Instance {
                    session,
                    active: BTreeMap::new(),
                    active_loaded: false,
                    rates: BTreeMap::new(),
                    rate: None,
                    context: None,
                    continuous: false,
                    interval_valid: false,
                    generation,
                    pending_end: None,
                    error: None,
                    pending: None,
                    worker: None,
                },
            );
        }
        let state = State {
            args: args.args,
            instances,
            current: None,
            pending_context: None,
            pending_start: None,
            pending_start_error: None,
            lost_lifecycle_error: None,
            summary: args.summary,
            details: args.details,
            stopped: args.stopped,
            status: args.status,
        };
        state.refresh_status();
        Ok(state)
    }
    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        // Root cancellation refuses new work. Exact exits finish operations already owned.
        let owned_exit =
            matches!(&message,Message::Exit(id,_,_) if state.instances.contains_key(id));
        if state.args.cancellation.is_cancelled() && !owned_exit {
            match message {
                Message::Select(_, Some(p))
                | Message::Bind(_, _, Some(p))
                | Message::Detach(_, Some(p))
                | Message::Context(_, _, p)
                | Message::Disconnected(_, _, _, p) => reply(p, Err(StoreError::Cancelled)),
                Message::Start(_, _, Some(p)) | Message::Session(_, p) => {
                    reply(p, Err(StoreError::Cancelled))
                }
                Message::Exit(_, _, Some(p)) | Message::Observe(_, p) => {
                    reply(p, Err(StoreError::Cancelled))
                }
                Message::CurrentSession(p) => reply(p, Err(StoreError::Cancelled)),
                Message::Connections(_, p) => reply(p, Err(StoreError::Cancelled)),
                Message::Usage(_, p) => reply(p, Err(StoreError::Cancelled)),
                Message::Topology(_, p) => reply(p, Err(StoreError::Cancelled)),
                #[cfg(test)]
                Message::ResidentActive(p) => reply(p, Err(StoreError::Cancelled)),
                _ => {}
            }
            return Ok(());
        }
        if !state.args.cancellation.is_cancelled() {
            state.retry_lifecycle(myself.clone()).await;
        }
        macro_rules! run {
            ($port:expr,$expr:expr) => {
                reply(
                    $port,
                    if state.args.cancellation.is_cancelled() {
                        Err(StoreError::Cancelled)
                    } else {
                        $expr
                    },
                )
            };
        }
        match message {
            #[cfg(test)]
            Message::ResidentActive(p) => reply(
                p,
                Ok(state
                    .instances
                    .values()
                    .map(|i| {
                        i.active.len()
                            + i.pending.as_ref().map_or(0, |pending| pending.active.len())
                    })
                    .sum()),
            ),
            Message::Select(id, port) => {
                state.current = id.clone();
                if let Some(id) = id.filter(|id| state.instances.contains_key(id)) {
                    state.publish(&id)
                } else {
                    state.summary.send_replace(None);
                    state.details.send_replace(None);
                }
                if let Some(port) = port {
                    reply(port, Ok(()));
                }
            }
            Message::Start(new, binding, port) => {
                let result = if state.args.cancellation.is_cancelled() {
                    Err(StoreError::Cancelled)
                } else {
                    state.start(new.clone(), binding.clone(), myself).await
                };
                if let Err(error) = &result
                    && !matches!(error, StoreError::InvalidData(_))
                {
                    state.lifecycle_failure(&new.instance_id, error);
                    if state.pending_start.is_none() {
                        state.pending_start = Some((new.clone(), binding, None));
                    } else {
                        state.lost_lifecycle_error = Some(StoreError::Unavailable(format!(
                            "multiple lifecycle starts could not be recorded; missing coverage includes {}: {error}",
                            new.instance_id
                        )));
                        state.refresh_status();
                    }
                }
                if let Some(p) = port {
                    reply(p, result)
                }
            }
            Message::Bind(binding, source, port) => {
                let result = if state.args.cancellation.is_cancelled() {
                    Err(StoreError::Cancelled)
                } else {
                    state.bind(binding.clone(), source, myself).await
                };
                if let Err(error) = &result
                    && !matches!(error, StoreError::Conflict(_))
                {
                    state.lifecycle_failure(&binding.instance_id, error);
                }
                if let Some(p) = port {
                    reply(p, result)
                }
            }
            Message::Exit(id, time, port) => {
                let result = state.exit(id.clone(), time).await;
                if let Err(error) = &result {
                    state.lifecycle_failure(&id, error);
                    if let Some((new, _, end)) = &mut state.pending_start
                        && new.instance_id == id
                    {
                        *end = Some(time)
                    }
                }
                if let Some(p) = port {
                    reply(p, result)
                }
            }
            Message::Detach(id, port) => {
                let result = state.detach(&id).await;
                if let Some(p) = port {
                    reply(p, result);
                }
            }
            Message::Observe(o, p) => run!(p, state.observe(o).await),
            Message::Disconnected(id, generation, reason, p) => {
                let result = if state.args.cancellation.is_cancelled() {
                    Err(StoreError::Cancelled)
                } else {
                    match state.instances.get_mut(&id) {
                        Some(i) if i.generation == generation => {
                            i.error = Some(reason.unwrap_or_else(|| {
                                StoreError::Unavailable("connections source disconnected".into())
                            }));
                            i.continuous = false;
                            i.interval_valid = false;
                            i.rate = None;
                            i.rates.clear();
                            i.session.freshness = Freshness::Stale;
                            accounting::add_quality(&mut i.session.quality, Quality::Gap);
                            Ok(())
                        }
                        _ => Err(StoreError::Conflict("stale disconnect".into())),
                    }
                };
                state.refresh_status();
                state.publish(&id);
                reply(p, result)
            }
            Message::Context(id, c, p) => run!(
                p,
                match state.instances.get_mut(&id) {
                    Some(i) => {
                        i.context = c;
                        Ok(())
                    }
                    None => {
                        state.pending_context = Some((id, c));
                        Ok(())
                    }
                }
            ),
            Message::CurrentSession(p) => {
                let selected = state
                    .current
                    .as_ref()
                    .and_then(|id| state.instances.get(id))
                    .map(|i| i.session.id.clone());
                let result = if let Some(id) = selected {
                    state.args.store.session(id).await.map(Some)
                } else if state.current.is_some() {
                    Err(StoreError::Unavailable(
                        "selected instance has no durable traffic session".into(),
                    ))
                } else {
                    state
                        .args
                        .store
                        .latest_session(state.args.host.clone())
                        .await
                };
                let result = result.map(|session| {
                    session.map(|mut session| {
                        state.overlay(&mut session);
                        session
                    })
                });
                run!(p, result);
            }
            Message::Session(id, p) => {
                let result = state.args.store.session(id).await.map(|mut session| {
                    state.overlay(&mut session);
                    session
                });
                run!(p, result);
            }
            Message::Connections(q, p) => {
                let result = state.args.store.query_connections(q).await.map(|mut page| {
                    state.overlay(&mut page.meta.session);
                    page
                });
                run!(p, result);
            }
            Message::Usage(q, p) => {
                let result = state.usage(q).await;
                run!(p, result);
            }
            Message::Topology(q, p) => {
                let result = state.topology(q).await;
                run!(p, result);
            }
        }
        Ok(())
    }
    async fn post_stop(
        &self,
        myself: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if !state.args.cancellation.is_cancelled() {
            state.retry_lifecycle(myself).await;
        }
        let mut failure = state
            .pending_start
            .as_ref()
            .map(|_| StoreError::Unavailable("unrecorded lifecycle at shutdown".into()));
        for i in state.instances.values_mut() {
            State::stop_worker(i).await;
            if let Some(p) = i.pending.take() {
                match state.args.store.commit_observation(p.commit.clone()).await {
                    Ok(_) => {
                        i.session = p.commit.session;
                        i.active = p.active
                    }
                    Err(e) => {
                        i.pending = Some(p);
                        failure = Some(e);
                    }
                }
            }
            if let Some(time) = i.pending_end
                && i.pending.is_none()
                && let Err(e) = state
                    .args
                    .store
                    .finish_session(SessionEnd {
                        session_id: i.session.id.clone(),
                        detected_at: time,
                        reason: CloseReason::CoreExited,
                    })
                    .await
            {
                failure = Some(e);
            }
        }
        let flushed = state.args.store.flush().await;
        let result = if let Some(error) = failure {
            Err(error)
        } else {
            flushed
        };
        state.stopped.send_replace(Some(result.clone()));
        result?;
        Ok(())
    }
}

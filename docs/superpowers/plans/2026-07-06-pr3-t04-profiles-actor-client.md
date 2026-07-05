# PR-3 T04 — ProfilesActor + ProfilesClient(核心事务)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** profiles 状态归属 `ProfilesActor`(owns `PersistentStateManager<Profiles>` + `ProfileDependencyIndex`);全部同步写消息(Add/Delete/Reorder/PatchMetadata/PatchRemoteOptions/ReplaceDefinition/SetCurrent/SetGlobalTransforms/Replace)+ Get 读;七步事务;`CommitReport{snapshot, affects_current, warnings}`。**不含** RefreshRemote/scheduler/watcher(→T05)。

**Architecture:** 严格沿用 PR-2b 实物样板——actor 形态照 `state/application.rs`(Args 注入、`manager.snapshot_handle().load()` 快照、`manager.upsert().await` 持久化);client 形态照 `client/application.rs`(client 构造器自建 manager 并 spawn actor、`CallResult` 三态映射、Drop stop、读 `Some(5s)`/写 `None`)。所有写 handler 走统一 `run_write` 事务助手:clone → mutate → `validate()` → 原子持久化 → 提交 + 重建索引 → post-commit 文件副作用(失败=降级 warning,不回滚,D5)→ `CommitReport`。

**Tech Stack:** ractor(已有)、`nyanpasu_core::state::{PersistentStateManager, PersistentStateManagerSetup}`、`nyanpasu_config::profile::*`(mutators/`ProfileDependencyIndex::build`/`validate`)、T03 三 ports(mock 注入)、nanoid 0.5(已有,服务端 uid)、camino `Utf8PathBuf`。

## Global Constraints(task.md §0)

- `state/profiles/**` + `client/profiles.rs` 禁止 `tauri::*` / `crate::config`(grep 断言)。
- 读 `call(_, Some(PROFILES_READ_TIMEOUT))`、写 `call(_, None)`;写 handler 禁无界 I/O(本卡全部写消息只做内存/本地 fs 操作,网络在 T05)。
- 测试不 sleep;mock ports + tempdir manager。
- 每个 commit `cargo build` + `cargo test` 绿。

## 基线事实(2026-07-06 实测)

- actor 样板:`state/application.rs:17-109`;client 样板:`client/application.rs:27-107`(含 `PersistentStateManagerSetup::<T>::builder().config_path(Utf8PathBuf).assemble()` → `load().await` / `from_state(seed).await`、`Drop → actor_ref.stop(None)`、tempdir 测试模式 `:109-164`)。
- 域 API:`Profiles::{get_item, append_item, replace_item, remove_item_unchecked, reorder, validate, sanitize_top_level}`(profiles.rs);mutators `ProfileItem::{apply_metadata_patch, set_definition}`、`Profiles::{set_current, add/remove/move_global_transform}`、`list_*`(patch.rs);`ProfileDependencyIndex::build(&Profiles)`(dependency.rs:25)。
- **`reorder_by_list` 域 API 不存在**——ByList 在 actor 内实现(置换校验 + IndexMap 重建)。
- `RemoteProfileOptionsPatch` 经 `struct_patch::Patch::apply` 应用到 `ProfileSource::Remote.option`。

## 契约修正(执行后回写 T04 卡,§5.3)

1. **Args 形态**:actor 不需要 `PathResolver`(fs 全走 ports;manager 由 client 构造器按 PR-2b 先例自建)——`ProfilesActorArgs{ manager, fs, fetcher, notifier }`;`ProfilesClient::new(profiles_path: Utf8PathBuf, fs, fetcher, notifier)`(fetcher 本卡仅存入 State 供 T05 使用)。
2. `CommitReport` 增加 `warnings: Vec<String>`(O4 落定:post-commit 副作用降级上报通道)。
3. `ProfilesError` 增加 `Persist(String)` 变体(持久化失败 ≠ Rpc 失败)。
4. **Add 的路径规约**:服务端生成 uid(`c`/`t` 前缀 + nanoid(11),按 definition 类别),并**强制重写** `definition.source.materialized.file` 为规范路径 `{uid}.{ext}`(File/Overlay→`yaml`,Script→按 runtime `js`/`lua`;请求内携带的占位路径一律忽略);External binding 的 `target` 保留请求值。
5. **加载策略**:client 构造时 `profiles_path` 存在→`load()`,否则 `from_state(Profiles::default())`;加载后 `validate()` 失败→构造失败(fail-fast;migration 保证合法,手改坏文件应显式失败而非静默兜底)。
6. **affects_current 规则表**(逐消息,均可测):Get n/a;Add=false;Delete=false(受引用保护,可删者必不在闭包内);Reorder=false;PatchMetadata=false;PatchRemoteOptions=false;SetCurrent=(前后 current 不等);SetGlobalTransforms=(前后列表不等);ReplaceDefinition=(闭包前后变化 ∨ uid ∈ 前/后闭包);Replace=true。闭包 = `current` + (Composition 的 base/extend 成员) + 上述各项的 scoped transforms + `global_transforms`;current=None 时 = `global_transforms`(bare 路径仍消费)。

---

### Task 1: 类型 + actor 骨架 + client 骨架(Get 通路)

**Files:**

- Create: `backend/tauri/src/state/profiles/actor.rs`
- Modify: `backend/tauri/src/state/profiles/mod.rs`(`pub mod actor;` + `pub use actor::*;`)
- Create: `backend/tauri/src/client/profiles.rs`
- Modify: `backend/tauri/src/client/mod.rs`(仅模块声明 `pub mod profiles;`,facade 方法留给 T07)

**Interfaces:**

- Produces(T05/T07/T08 依赖,冻结):

```rust
pub const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);   // client/profiles.rs
pub struct CommitReport { pub snapshot: Arc<Profiles>, pub affects_current: bool, pub warnings: Vec<String> }
pub struct NewProfileRequest { pub metadata: ProfileMetadata, pub definition: ProfileDefinition }
pub enum ReorderOp { Move { active: ProfileId, over: ProfileId }, ByList(Vec<ProfileId>) }
pub enum ProfilesError { ProfileNotFound(ProfileId), ProfileInUse { referrers: Vec<ProfileId> },
    ProfileHasNoFile, ValidationFailed(Vec<ProfileValidationError>), NotARemoteProfile,
    FileNotWritable { reason: String }, RefreshFailed { message: String },
    Persist(String), Rpc(String) }
impl ProfilesClient {
    pub(crate) async fn new(profiles_path: Utf8PathBuf, fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>, notifier: Arc<dyn RebuildNotifier>) -> anyhow::Result<Self>;
    pub async fn get(&self) -> Result<Arc<Profiles>, ProfilesError>;
    // 九个写方法 → Result<CommitReport, ProfilesError>(Task 2–4 逐个接通)
}
```

- [ ] **Step 1: 写失败测试(client/profiles.rs 底部)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::{
        MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher,
    };
    use tempfile::{TempDir, tempdir};

    pub(crate) fn temp_profiles_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).expect("utf-8 temp path")
    }

    pub(crate) async fn test_client_with(fs: MockProfileFsPort) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("profiles client should spawn");
        (client, dir)
    }

    #[tokio::test]
    async fn fresh_store_starts_with_default_profiles() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let snapshot = client.get().await.expect("get should succeed");
        assert!(snapshot.current.is_none());
        assert!(snapshot.items.is_empty());
        assert_eq!(snapshot.valid.len(), 3); // default_valid
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu fresh_store_starts`
Expected: FAIL(类型未定义)。

- [ ] **Step 3: 实现 `state/profiles/actor.rs`(本 Task 仅 Get + 骨架;写消息枚举齐全但 handler 返回 todo 错误会违反「每步编译且测试绿」——因此消息枚举本 Task 只含 `Get`,后续 Task 增量添加变体)**

```rust
//! ProfilesActor: single owner of the profiles document (design §6).
//! Tauri-free (D10); every filesystem/network effect goes through the ports.

use std::sync::Arc;

use nyanpasu_config::profile::{
    ConfigDefinition, ProfileDefinition, ProfileDependencyIndex, ProfileId, ProfileMetadata,
    ProfileValidationError, Profiles,
};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use super::ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher};

#[derive(Debug, thiserror::Error)]
pub enum ProfilesError {
    #[error("profile not found: {0}")]
    ProfileNotFound(ProfileId),
    #[error("profile is referenced and cannot be deleted (referrers: {referrers:?})")]
    ProfileInUse { referrers: Vec<ProfileId> },
    #[error("profile has no materialized file")]
    ProfileHasNoFile,
    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<ProfileValidationError>),
    #[error("profile is not a remote profile")]
    NotARemoteProfile,
    #[error("file not writable: {reason}")]
    FileNotWritable { reason: String },
    #[error("refresh failed: {message}")]
    RefreshFailed { message: String },
    #[error("failed to persist profiles: {0}")]
    Persist(String),
    #[error("profiles actor rpc failed: {0}")]
    Rpc(String),
}

#[derive(Debug, Clone)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
    /// Dependency-closure judgement per the T04 affects_current rule table.
    pub affects_current: bool,
    /// Post-commit side-effect failures (degraded, not rolled back — D5).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewProfileRequest {
    pub metadata: ProfileMetadata,
    /// The materialized path inside is a placeholder: Add rewrites it to the
    /// canonical `{uid}.{ext}` derived from the server-generated uid.
    pub definition: ProfileDefinition,
}

#[derive(Debug, Clone)]
pub enum ReorderOp {
    Move { active: ProfileId, over: ProfileId },
    ByList(Vec<ProfileId>),
}

pub struct ProfilesActorArgs {
    pub manager: PersistentStateManager<Profiles>,
    pub fs: Arc<dyn ProfileFsPort>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub notifier: Arc<dyn RebuildNotifier>,
}

pub struct ProfilesActorState {
    manager: PersistentStateManager<Profiles>,
    index: ProfileDependencyIndex,
    fs: Arc<dyn ProfileFsPort>,
    // T05 consumes these; stored now so Args stay stable across T04/T05.
    #[allow(dead_code)]
    fetcher: Arc<dyn SubscriptionFetcher>,
    #[allow(dead_code)]
    notifier: Arc<dyn RebuildNotifier>,
}

#[derive(Debug)]
pub enum ProfilesActorMessage {
    Get(RpcReplyPort<Result<Arc<Profiles>, ProfilesError>>),
    // 写消息变体由 Task 2–4 增量补齐
}

pub struct ProfilesActor;

impl ProfilesActor {
    fn current_state(state: &ProfilesActorState) -> Profiles {
        state.manager.snapshot_handle().load().state.clone()
    }
}

impl Actor for ProfilesActor {
    type Msg = ProfilesActorMessage;
    type State = ProfilesActorState;
    type Arguments = ProfilesActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let index = ProfileDependencyIndex::build(&args.manager.snapshot_handle().load().state);
        Ok(ProfilesActorState {
            manager: args.manager,
            index,
            fs: args.fs,
            fetcher: args.fetcher,
            notifier: args.notifier,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ProfilesActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Arc::new(Self::current_state(state))));
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: 实现 `client/profiles.rs`**

```rust
//! Typed client for the ProfilesActor (design §6). Read Some(5s) / write None.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::Profiles;
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::profiles::{
    ProfilesActor, ProfilesActorArgs, ProfilesActorMessage, ProfilesError,
    ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher},
};

pub const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
}

impl ProfilesClient {
    pub(crate) async fn new(
        profiles_path: Utf8PathBuf,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        notifier: Arc<dyn RebuildNotifier>,
    ) -> anyhow::Result<Self> {
        let should_load = profiles_path.exists();
        let setup = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(profiles_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load profiles persistent state manager")?
        } else {
            setup
                .from_state(Profiles::default())
                .await
                .context("failed to initialize profiles persistent state manager")?
        };

        // Fail fast on hand-edited invalid documents: migration guarantees a
        // valid file, so anything else is an explicit error, not a fallback.
        manager
            .snapshot_handle()
            .load()
            .state
            .validate()
            .map_err(|errors| anyhow::anyhow!("profiles.yaml failed validation: {errors:?}"))?;

        let actor_ref = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorArgs {
                manager,
                fs,
                fetcher,
                notifier,
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ProfilesClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> Result<Arc<Profiles>, ProfilesError> {
        self.call(ProfilesActorMessage::Get, Some(PROFILES_READ_TIMEOUT))
            .await
    }

    async fn call<F, T>(&self, make: F, timeout: Option<Duration>) -> Result<T, ProfilesError>
    where
        F: FnOnce(RpcReplyPort<Result<T, ProfilesError>>) -> ProfilesActorMessage,
        T: Send + 'static,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) => Err(ProfilesError::Rpc("reply dropped".into())),
            Ok(CallResult::Timeout) => Err(ProfilesError::Rpc("call timed out".into())),
            Err(e) => Err(ProfilesError::Rpc(e.to_string())),
        }
    }
}

impl Drop for ProfilesClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}
```

`state/profiles/mod.rs` 更新为:

```rust
//! Profiles domain state (PR-3). Tauri-free (D10).

pub mod actor;
pub mod ports;

pub use actor::*;
```

`client/mod.rs` 声明区追加 `pub mod profiles;`。

- [ ] **Step 5: 跑测试确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu fresh_store_starts`
Expected: PASS。

```bash
git add backend/tauri/src/state/profiles backend/tauri/src/client/profiles.rs backend/tauri/src/client/mod.rs
git commit -m "feat(tauri): scaffold ProfilesActor and ProfilesClient with read path"
```

---

### Task 2: 事务助手 + SetCurrent / SetGlobalTransforms / Replace

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`、`backend/tauri/src/client/profiles.rs`

- [ ] **Step 1: 写失败测试**

```rust
    use nyanpasu_config::profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        OverlayTransform, ProfileDefinition, ProfileId, ProfileMetadata, ProfileSource,
        Profiles, TransformDefinition,
    };

    pub(crate) fn file_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata { name: uid.to_uppercase(), desc: None },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    pub(crate) fn overlay_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata { name: uid.to_uppercase(), desc: None },
            definition: ProfileDefinition::Transform {
                transform: TransformDefinition::Overlay(OverlayTransform {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                }),
            },
        }
    }

    pub(crate) fn seeded_profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg1"));
        profiles.append_item(file_config_item("cfg2"));
        profiles.append_item(overlay_item("ovl1"));
        profiles
    }

    async fn seeded_client() -> (ProfilesClient, TempDir) {
        let (client, dir) = test_client_with(MockProfileFsPort::new()).await;
        client.replace(seeded_profiles()).await.expect("seed replace");
        (client, dir)
    }

    #[tokio::test]
    async fn set_current_commits_and_reports_affects_current() {
        let (client, dir) = seeded_client().await;
        let report = client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .expect("activate cfg1");
        assert!(report.affects_current);
        assert_eq!(report.snapshot.current, Some(ProfileId("cfg1".into())));

        // 同值再设 → 不影响 current
        let report = client.set_current(Some(ProfileId("cfg1".into()))).await.unwrap();
        assert!(!report.affects_current);

        // 落盘验证:重启后仍在
        drop(client);
        let (client, _dir2) = {
            let path = temp_profiles_path(&dir);
            let client = ProfilesClient::new(
                path,
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                std::sync::Arc::new(MockRebuildNotifier::new()),
            )
            .await
            .unwrap();
            (client, dir)
        };
        assert_eq!(client.get().await.unwrap().current, Some(ProfileId("cfg1".into())));
    }

    #[tokio::test]
    async fn set_current_rejects_missing_and_transform_targets() {
        let (client, _dir) = seeded_client().await;
        let err = client.set_current(Some(ProfileId("ghost".into()))).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        let err = client.set_current(Some(ProfileId("ovl1".into()))).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        // 失败不落盘不改内存
        assert!(client.get().await.unwrap().current.is_none());
    }

    #[tokio::test]
    async fn set_global_transforms_validates_kind_and_reports_change() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .set_global_transforms(vec![ProfileId("ovl1".into())])
            .await
            .expect("set transforms");
        assert!(report.affects_current);
        // Config uid 进 global_transforms → ValidationFailed(design §8.2)
        let err = client
            .set_global_transforms(vec![ProfileId("cfg1".into())])
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu set_current set_global`
Expected: FAIL(方法未定义)。

- [ ] **Step 3: 实现事务助手 + 三消息**

actor.rs 追加(核心事务助手,后续全部写消息复用):

```rust
/// Post-commit filesystem side effects (executed after the state is durable;
/// failures degrade into CommitReport.warnings — D5).
pub(super) enum PostCommitOp {
    WriteInitial { path: nyanpasu_config::profile::ManagedProfilePath, content: String },
    Remove { path: nyanpasu_config::profile::ManagedProfilePath },
    EnsureSymlink {
        path: nyanpasu_config::profile::ManagedProfilePath,
        target: nyanpasu_config::profile::ExternalProfilePath,
    },
}

pub(super) struct WriteOutcome {
    pub affects: AffectsRule,
    pub post_ops: Vec<PostCommitOp>,
}

/// affects_current rule table (T04 contract addendum #6).
pub(super) enum AffectsRule {
    Never,
    CurrentChanged,
    GlobalChanged,
    Touched(ProfileId),
    Always,
}

impl ProfilesActor {
    /// current 的传递闭包:current + Composition 成员 + 各自 scoped transforms
    /// + global transforms;current=None 时 = global transforms(bare 路径)。
    fn current_closure(profiles: &Profiles) -> indexmap::IndexSet<ProfileId> {
        let mut closure: indexmap::IndexSet<ProfileId> =
            profiles.global_transforms.iter().cloned().collect();
        let Some(current) = &profiles.current else {
            return closure;
        };
        closure.insert(current.clone());
        let mut configs = vec![current.clone()];
        if let Some(item) = profiles.items.get(current) {
            if let ProfileDefinition::Config {
                config: ConfigDefinition::Composition(composition),
            } = &item.definition
            {
                if let Some(base) = &composition.base {
                    closure.insert(base.clone());
                    configs.push(base.clone());
                }
                for member in &composition.extend_proxies_from {
                    closure.insert(member.clone());
                    configs.push(member.clone());
                }
            }
        }
        for config in configs {
            if let Some(item) = profiles.items.get(&config) {
                if let ProfileDefinition::Config { config } = &item.definition {
                    for transform in config.transforms() {
                        closure.insert(transform.clone());
                    }
                }
            }
        }
        closure
    }

    fn evaluate_affects(rule: &AffectsRule, before: &Profiles, after: &Profiles) -> bool {
        match rule {
            AffectsRule::Never => false,
            AffectsRule::Always => true,
            AffectsRule::CurrentChanged => before.current != after.current,
            AffectsRule::GlobalChanged => before.global_transforms != after.global_transforms,
            AffectsRule::Touched(uid) => {
                let closure_before = Self::current_closure(before);
                let closure_after = Self::current_closure(after);
                closure_before != closure_after
                    || closure_before.contains(uid)
                    || closure_after.contains(uid)
            }
        }
    }

    /// Seven-step transaction (design §6.3): clone → mutate → validate →
    /// persist → commit + rebuild index → post-commit side effects → report.
    async fn run_write<F>(
        state: &mut ProfilesActorState,
        mutate: F,
    ) -> Result<CommitReport, ProfilesError>
    where
        F: FnOnce(&mut Profiles) -> Result<WriteOutcome, ProfilesError>,
    {
        let before = Self::current_state(state);
        let mut next = before.clone();
        let outcome = mutate(&mut next)?;
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        state
            .manager
            .upsert(next.clone())
            .await
            .map_err(|e| ProfilesError::Persist(e.to_string()))?;
        state.index = ProfileDependencyIndex::build(&next);

        let mut warnings = Vec::new();
        for op in outcome.post_ops {
            let result = match &op {
                PostCommitOp::WriteInitial { path, content } => state.fs.write_atomic(path, content),
                PostCommitOp::Remove { path } => state.fs.remove(path),
                PostCommitOp::EnsureSymlink { path, target } => state.fs.ensure_symlink(path, target),
            };
            if let Err(error) = result {
                warnings.push(format!("post-commit file operation failed: {error}"));
            }
        }

        let affects_current = Self::evaluate_affects(&outcome.affects, &before, &next);
        Ok(CommitReport {
            snapshot: Arc::new(next),
            affects_current,
            warnings,
        })
    }
}
```

消息变体 + handler 分支(追加到既有 enum/match):

```rust
    SetCurrent {
        current: Option<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetGlobalTransforms {
        ids: Vec<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Replace {
        profiles: Profiles,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
```

```rust
            ProfilesActorMessage::SetCurrent { current, reply } => {
                let result = Self::run_write(state, |profiles| {
                    profiles.set_current(current);
                    Ok(WriteOutcome { affects: AffectsRule::CurrentChanged, post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetGlobalTransforms { ids, reply } => {
                let result = Self::run_write(state, |profiles| {
                    profiles.global_transforms = ids;
                    Ok(WriteOutcome { affects: AffectsRule::GlobalChanged, post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Replace { profiles: next, reply } => {
                let result = Self::run_write(state, |profiles| {
                    *profiles = next;
                    Ok(WriteOutcome { affects: AffectsRule::Always, post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
```

client 方法(全部写 = `call(_, None)`):

```rust
    pub async fn set_current(&self, current: Option<ProfileId>) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::SetCurrent { current, reply }, None).await
    }

    pub async fn set_global_transforms(&self, ids: Vec<ProfileId>) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::SetGlobalTransforms { ids, reply }, None).await
    }

    pub async fn replace(&self, profiles: Profiles) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Replace { profiles, reply }, None).await
    }
```

- [ ] **Step 4: 跑测试确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu set_current set_global fresh_store`
Expected: 全 PASS。

```bash
git add backend/tauri/src/state/profiles/actor.rs backend/tauri/src/client/profiles.rs
git commit -m "feat(tauri): add profiles write transaction with current/global/replace"
```

---

### Task 3: Add + Delete(uid 生成、规范路径、引用保护、按 binding 清理)

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`、`backend/tauri/src/client/profiles.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn add_generates_uid_canonical_path_and_writes_initial_file() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str().ends_with(".yaml") && content == "proxies: []\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;

        let report = client
            .add(
                NewProfileRequest {
                    metadata: ProfileMetadata { name: "New".into(), desc: None },
                    definition: file_config_item("placeholder").definition,
                },
                Some("proxies: []\n".to_string()),
            )
            .await
            .expect("add should succeed");

        assert!(!report.affects_current); // Add 规则:false
        assert!(report.warnings.is_empty());
        let snapshot = report.snapshot;
        assert_eq!(snapshot.items.len(), 1);
        let (uid, item) = snapshot.items.first().unwrap();
        assert!(uid.0.starts_with('c'), "config uid prefixed with c: {uid}");
        // 规范路径 = {uid}.yaml,请求内的 placeholder 路径被忽略
        let file = item.definition.source().unwrap().materialized().file.as_str();
        assert_eq!(file, format!("{uid}.yaml"));
    }

    #[tokio::test]
    async fn add_script_transform_uses_runtime_extension() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_write_atomic()
            .withf(|path, _| path.as_str().ends_with(".lua"))
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut item = overlay_item("placeholder");
        item.definition = ProfileDefinition::Transform {
            transform: TransformDefinition::Script(nyanpasu_config::profile::ScriptTransform {
                source: overlay_item("p").definition.source().unwrap().clone(),
                runtime: nyanpasu_config::profile::ScriptRuntime::Lua,
            }),
        };
        let report = client
            .add(
                NewProfileRequest { metadata: item.metadata.clone(), definition: item.definition },
                Some("-- lua".to_string()),
            )
            .await
            .unwrap();
        let (uid, _) = report.snapshot.items.first().unwrap();
        assert!(uid.0.starts_with('t'), "transform uid prefixed with t");
    }

    #[tokio::test]
    async fn delete_enforces_reference_protection() {
        let (client, _dir) = seeded_client().await;
        // current 引用
        client.set_current(Some(ProfileId("cfg1".into()))).await.unwrap();
        let err = client.delete(ProfileId("cfg1".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileInUse { .. }));
        // global transform 引用
        client.set_global_transforms(vec![ProfileId("ovl1".into())]).await.unwrap();
        let err = client.delete(ProfileId("ovl1".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileInUse { .. }));
        // 不存在
        let err = client.delete(ProfileId("ghost".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileNotFound(_)));
    }

    #[tokio::test]
    async fn delete_unreferenced_managed_profile_removes_file() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .withf(|path| path.as_str() == "cfg2.yaml")
            .times(1)
            .returning(|_| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        client.replace(seeded_profiles()).await.unwrap();
        let report = client.delete(ProfileId("cfg2".into())).await.expect("delete cfg2");
        assert!(!report.affects_current);
        assert!(report.snapshot.items.get(&ProfileId("cfg2".into())).is_none());
    }

    #[tokio::test]
    async fn delete_cleanup_failure_degrades_to_warning() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove().returning(|_| anyhow::bail!("disk on fire"));
        let (client, _dir) = test_client_with(fs).await;
        client.replace(seeded_profiles()).await.unwrap();
        let report = client.delete(ProfileId("cfg2".into())).await.expect("delete commits anyway");
        assert_eq!(report.warnings.len(), 1); // 降级不回滚(D5)
        assert!(report.snapshot.items.get(&ProfileId("cfg2".into())).is_none());
    }
```

(引用保护的 base/extend/scoped-transform 三类由 Task 4 的 ReplaceDefinition 测试补齐场景后追加同构断言;本 Task 先覆盖 current/global/不存在三类。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu add_ delete_`
Expected: FAIL。

- [ ] **Step 3: 实现 Add/Delete**

消息变体:

```rust
    Add {
        request: NewProfileRequest,
        initial_file: Option<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Delete {
        uid: ProfileId,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
```

actor 逻辑:

```rust
impl ProfilesActor {
    fn generate_uid(definition: &ProfileDefinition, existing: &Profiles) -> ProfileId {
        let prefix = match definition {
            ProfileDefinition::Config { .. } => 'c',
            ProfileDefinition::Transform { .. } => 't',
        };
        loop {
            let candidate = ProfileId(format!("{prefix}{}", nanoid::nanoid!(11)));
            if existing.items.get(&candidate).is_none() {
                return candidate;
            }
        }
    }

    fn canonical_extension(definition: &ProfileDefinition) -> &'static str {
        match definition {
            ProfileDefinition::Config { .. } => "yaml",
            ProfileDefinition::Transform { transform } => match transform {
                nyanpasu_config::profile::TransformDefinition::Overlay(_) => "yaml",
                nyanpasu_config::profile::TransformDefinition::Script(script) => {
                    match script.runtime {
                        nyanpasu_config::profile::ScriptRuntime::JavaScript => "js",
                        nyanpasu_config::profile::ScriptRuntime::Lua => "lua",
                    }
                }
            },
        }
    }

    /// design §17 五类引用;referrers 装 item 级引用者,current/global 命中时
    /// 也返回 ProfileInUse(引用者为文档级,不进列表)。
    fn referrers_of(state: &ProfilesActorState, profiles: &Profiles, uid: &ProfileId) -> Option<Vec<ProfileId>> {
        let mut referrers: indexmap::IndexSet<ProfileId> = Default::default();
        if let Some(set) = state.index.composition_base_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.extend_proxies_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.transform_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        let document_level = profiles.current.as_ref() == Some(uid)
            || state.index.global_transform_ids.contains(uid);
        if referrers.is_empty() && !document_level {
            None
        } else {
            Some(referrers.into_iter().collect())
        }
    }
}
```

handler 分支:

```rust
            ProfilesActorMessage::Add { request, initial_file, reply } => {
                let result = {
                    let existing = Self::current_state(state);
                    let uid = Self::generate_uid(&request.definition, &existing);
                    let ext = Self::canonical_extension(&request.definition);
                    let canonical =
                        nyanpasu_config::profile::ManagedProfilePath::new(format!("{uid}.{ext}"))
                            .expect("uid-derived path is always a valid managed path");
                    let mut definition = request.definition;
                    let mut post_ops = Vec::new();
                    if let Some(source) = definition.source_mut() {
                        source.materialized_mut().file = canonical.clone();
                        match source {
                            nyanpasu_config::profile::ProfileSource::Local {
                                binding:
                                    nyanpasu_config::profile::LocalBinding::External { target, mode, .. },
                            } => {
                                if *mode == nyanpasu_config::profile::ExternalMode::Symlink {
                                    post_ops.push(PostCommitOp::EnsureSymlink {
                                        path: canonical.clone(),
                                        target: target.clone(),
                                    });
                                }
                                // Mirror 的首次同步由 T05 watcher reconcile 负责
                            }
                            nyanpasu_config::profile::ProfileSource::Remote { .. } => {
                                // 首次内容由 T05 RefreshRemote 链路下载
                            }
                            _ => {
                                post_ops.push(PostCommitOp::WriteInitial {
                                    path: canonical.clone(),
                                    content: initial_file.clone().unwrap_or_default(),
                                });
                            }
                        }
                    }
                    let item = nyanpasu_config::profile::ProfileItem {
                        uid: uid.clone(),
                        metadata: request.metadata,
                        definition,
                    };
                    Self::run_write(state, move |profiles| {
                        if !profiles.append_item(item) {
                            return Err(ProfilesError::Persist("uid collision".into()));
                        }
                        Ok(WriteOutcome { affects: AffectsRule::Never, post_ops })
                    })
                    .await
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let result = {
                    let existing = Self::current_state(state);
                    if existing.items.get(&uid).is_none() {
                        Err(ProfilesError::ProfileNotFound(uid.clone()))
                    } else if let Some(referrers) = Self::referrers_of(state, &existing, &uid) {
                        Err(ProfilesError::ProfileInUse { referrers })
                    } else {
                        let removed = existing.items.get(&uid).cloned();
                        let post_ops = removed
                            .as_ref()
                            .and_then(|item| item.definition.source())
                            .map(|source| {
                                // 图 13.5:Managed/Remote 删物化文件;External Symlink
                                // 只删应用管理的链接;Mirror 删副本;target 永不动
                                vec![PostCommitOp::Remove {
                                    path: source.materialized().file.clone(),
                                }]
                            })
                            .unwrap_or_default(); // Composition:无文件操作
                        Self::run_write(state, move |profiles| {
                            profiles.remove_item_unchecked(&uid);
                            Ok(WriteOutcome { affects: AffectsRule::Never, post_ops })
                        })
                        .await
                    }
                };
                let _ = reply.send(result);
            }
```

client 方法:

```rust
    pub async fn add(&self, request: NewProfileRequest, initial_file: Option<String>) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Add { request, initial_file, reply }, None).await
    }

    pub async fn delete(&self, uid: ProfileId) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Delete { uid, reply }, None).await
    }
```

- [ ] **Step 4: 跑测试确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu add_ delete_ set_ fresh_store`
Expected: 全 PASS。

```bash
git add backend/tauri/src/state/profiles/actor.rs backend/tauri/src/client/profiles.rs
git commit -m "feat(tauri): add profile create/delete with reference protection"
```

---

### Task 4: Reorder + PatchMetadata + PatchRemoteOptions + ReplaceDefinition

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`、`backend/tauri/src/client/profiles.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn reorder_move_and_by_list() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .reorder(ReorderOp::Move { active: ProfileId("cfg2".into()), over: ProfileId("cfg1".into()) })
            .await
            .unwrap();
        assert!(!report.affects_current);
        let uids: Vec<_> = report.snapshot.items.keys().map(|u| u.0.clone()).collect();
        assert_eq!(uids, vec!["cfg2", "cfg1", "ovl1"]);

        let report = client
            .reorder(ReorderOp::ByList(vec![
                ProfileId("ovl1".into()), ProfileId("cfg1".into()), ProfileId("cfg2".into()),
            ]))
            .await
            .unwrap();
        let uids: Vec<_> = report.snapshot.items.keys().map(|u| u.0.clone()).collect();
        assert_eq!(uids, vec!["ovl1", "cfg1", "cfg2"]);

        // 非置换列表 → 错误(缺元素)
        let err = client.reorder(ReorderOp::ByList(vec![ProfileId("cfg1".into())])).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_) | ProfilesError::ProfileNotFound(_)));
    }

    #[tokio::test]
    async fn patch_metadata_and_remote_options() {
        let (client, _dir) = seeded_client().await;
        let mut patch = ProfileMetadata::new_empty_patch();
        patch.name = Some("Renamed".into());
        let report = client.patch_metadata(ProfileId("cfg1".into()), patch).await.unwrap();
        assert!(!report.affects_current);
        assert_eq!(report.snapshot.items[&ProfileId("cfg1".into())].metadata.name, "Renamed");

        // 非 Remote → NotARemoteProfile
        let options_patch = nyanpasu_config::profile::RemoteProfileOptions::new_empty_patch();
        let err = client
            .patch_remote_options(ProfileId("cfg1".into()), options_patch)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::NotARemoteProfile));
    }

    #[tokio::test]
    async fn replace_definition_is_atomic_and_reports_closure_hit() {
        let (client, _dir) = seeded_client().await;
        client.set_current(Some(ProfileId("cfg1".into()))).await.unwrap();

        // cfg1(current)追加 scoped transform → 闭包变化 → affects_current
        let mut definition = seeded_profiles().items[&ProfileId("cfg1".into())].definition.clone();
        if let ProfileDefinition::Config { config: ConfigDefinition::File(file) } = &mut definition {
            file.transforms = vec![ProfileId("ovl1".into())];
        }
        let report = client
            .replace_definition(ProfileId("cfg1".into()), definition)
            .await
            .unwrap();
        assert!(report.affects_current);

        // 闭包外的 cfg2 变更 → 不影响
        let definition = seeded_profiles().items[&ProfileId("cfg2".into())].definition.clone();
        let report = client.replace_definition(ProfileId("cfg2".into()), definition).await.unwrap();
        assert!(!report.affects_current);

        // scoped transform 被引用后,删除 ovl1 → ProfileInUse{referrers=[cfg1]}
        let err = client.delete(ProfileId("ovl1".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse { referrers } => {
                assert_eq!(referrers, vec![ProfileId("cfg1".into())]);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu reorder_ patch_metadata replace_definition`
Expected: FAIL。

- [ ] **Step 3: 实现四消息**

消息变体:

```rust
    Reorder { op: ReorderOp, reply: RpcReplyPort<Result<CommitReport, ProfilesError>> },
    PatchMetadata {
        uid: ProfileId,
        patch: nyanpasu_config::profile::ProfileMetadataPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchRemoteOptions {
        uid: ProfileId,
        patch: nyanpasu_config::profile::RemoteProfileOptionsPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    ReplaceDefinition {
        uid: ProfileId,
        definition: ProfileDefinition,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
```

handler 分支:

```rust
            ProfilesActorMessage::Reorder { op, reply } => {
                let result = Self::run_write(state, move |profiles| {
                    match op {
                        ReorderOp::Move { active, over } => {
                            if profiles.items.get(&active).is_none() {
                                return Err(ProfilesError::ProfileNotFound(active));
                            }
                            if profiles.items.get(&over).is_none() {
                                return Err(ProfilesError::ProfileNotFound(over));
                            }
                            profiles.reorder(&active, &over);
                        }
                        ReorderOp::ByList(list) => {
                            // 必须是现有 uid 集的一个置换
                            if list.len() != profiles.items.len() {
                                return Err(ProfilesError::ValidationFailed(vec![]));
                            }
                            let mut reordered = indexmap::IndexMap::with_capacity(list.len());
                            for uid in list {
                                let Some(item) = profiles.items.shift_remove(&uid) else {
                                    return Err(ProfilesError::ProfileNotFound(uid));
                                };
                                reordered.insert(uid, item);
                            }
                            profiles.items = reordered;
                        }
                    }
                    Ok(WriteOutcome { affects: AffectsRule::Never, post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchMetadata { uid, patch, reply } => {
                let result = Self::run_write(state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    item.apply_metadata_patch(patch);
                    Ok(WriteOutcome { affects: AffectsRule::Never, post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply } => {
                let result = Self::run_write(state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    match item.definition.source_mut() {
                        Some(nyanpasu_config::profile::ProfileSource::Remote { option, .. }) => {
                            use struct_patch::Patch as _;
                            option.apply(patch);
                            Ok(WriteOutcome { affects: AffectsRule::Never, post_ops: vec![] })
                        }
                        _ => Err(ProfilesError::NotARemoteProfile),
                    }
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ReplaceDefinition { uid, definition, reply } => {
                let result = Self::run_write(state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid.clone()));
                    };
                    item.set_definition(definition);
                    Ok(WriteOutcome { affects: AffectsRule::Touched(uid), post_ops: vec![] })
                })
                .await;
                let _ = reply.send(result);
            }
```

client 方法(签名同 T04 卡 Produces,全部 `call(_, None)`):`reorder`、`patch_metadata`、`patch_remote_options`、`replace_definition`。

(注:ByList 的空 `ValidationFailed(vec![])` 若语义不适,可在 `ProfilesError` 增 `InvalidReorderList` 变体——执行时二选一并回写卡。)

- [ ] **Step 4: 跑全部 T04 测试 + 纯度断言 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --lib profiles`
Expected: 全 PASS。
Run(Git Bash): `grep -rn "tauri::\|crate::config" backend/tauri/src/state/profiles/ backend/tauri/src/client/profiles.rs && echo LEAK || echo CLEAN`
Expected: `CLEAN`。
Run(Git Bash): `grep -n "call(" backend/tauri/src/client/profiles.rs` — 人工核对:唯一 `Some(PROFILES_READ_TIMEOUT)` 出现在 `get`,其余全为 `None`。

```bash
git add backend/tauri/src/state/profiles/actor.rs backend/tauri/src/client/profiles.rs
git commit -m "feat(tauri): complete profiles actor synchronous write protocol"
```

---

### Task 5: 契约回写 + 全量回归

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T04 卡)

- [ ] **Step 1: 全量回归**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo clippy --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --all-targets --all-features`
Expected: 全绿(clippy 无新警告)。

- [ ] **Step 2: 按「契约修正」节 1–6 更新 T04 卡(Args/CommitReport.warnings/Persist 变体/Add 路径规约/加载策略/affects_current 规则表),并同步 T05/T07 卡的 Consumes 引用**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T04 contract addenda (args, warnings, affects rules)"
```

---

## 验证总表(对应 T04 卡验证段)

| 判据                                       | 覆盖                                                   |
| ------------------------------------------ | ------------------------------------------------------ |
| 每条消息 happy path                        | Task 2/3/4 逐消息测试                                  |
| `ValidationFailed` 不落盘不改内存          | Task 2 `set_current_rejects_...`(重启持久性另证)       |
| Delete 引用保护(current/global/item 级)    | Task 3 `delete_enforces_...` + Task 4 referrers 断言   |
| `affects_current` 判定(闭包内/外)          | Task 2 SetCurrent/SetGlobal + Task 4 ReplaceDefinition |
| Add 写初始文件、Delete 按 binding 清理     | Task 3 mock 期望(Managed/清理失败降级)                 |
| 写 `call(_,None)`、读 `call(_,Some)`       | Task 4 Step 4 grep 核对                                |
| 不 sleep                                   | 全部测试经 RpcReplyPort ack                            |
| `cargo test -p clash-nyanpasu profiles` 绿 | Task 4 Step 4 / Task 5 Step 1                          |

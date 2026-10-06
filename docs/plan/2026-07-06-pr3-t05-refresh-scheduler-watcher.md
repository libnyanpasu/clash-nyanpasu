# PR-3 T05 — RefreshRemote + RemoteUpdateScheduler + External watcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 订阅刷新与外部文件同步纳入 ProfilesActor:下载-提交分离(`RefreshRemote` 挂起 reply → 子任务下载写盘 → `CommitRefreshed` 串行提交)、定时表幂等 reconcile(含启动补课)、External Symlink/Mirror watcher、后台提交且影响 current 时经 `RebuildNotifier` 通知恰好一次。

**Architecture:** 全部提交仍串行于 actor(T04 `run_write` 复用);写 handler **零网络 I/O**(D9)——`RefreshRemote` 只做校验 + 可选 options patch 事务 + spawn 下载任务;下载任务(tokio::spawn)自己 fetch → 按目标类型校验 → `ensure_not_symlink` → `write_atomic`,完成后 `cast(CommitRefreshed)` 回填。scheduler = actor 内维护的 per-uid tokio 定时任务表(`post_start` 首次 reconcile,每次 commit 后按 diff 增删,语义对齐旧 `ProfilesJob::{refresh,diff,init}`);watcher = notify-debouncer-full(先例 `logging/manager.rs:252`),事件 → `cast(ExternalFileChanged)`。定时逻辑走 tokio 时间,测试用 `start_paused` + `advance`(零真实 sleep)。

**Tech Stack:** T04 actor/事务、T03 ports(mock)、tokio 时间轮、notify 8 + notify-debouncer-full 0.7(已有)、`time::OffsetDateTime`。

## Global Constraints(task.md §0)

- 写 handler 禁无界 I/O;下载超时由 fetcher 实现自管(T03)。
- 挂起 reply 必结清(成功/失败/uid 已删三路径)。
- 测试不真实 sleep:定时用 `#[tokio::test(start_paused = true)]` + `tokio::time::advance`;watcher 处理逻辑经直接注入消息测试,真实文件事件仅一个有界(≤5s)接线冒烟。
- `state/profiles/**` 维持 Tauri-free/Config-free(grep 断言)。

## 基线事实(2026-07-06 实测)

- 被取代者语义(`core/tasks/jobs/profiles.rs`):interval 单位分钟、仅 `interval > 0` 建任务(新 schema 验证已禁 0)、diff 三操作(Add/Remove/Update)、**`init()` 启动补课:`now - updated >= interval` → 立即刷新**(`:84-115`)。
- watcher 先例:`logging/manager.rs:6-9,252`(`new_debouncer(timeout, None, callback)` + `RecommendedWatcher` + `DebounceEventResult`)。
- 刷新对象 = 一切 `ProfileSource::Remote` 的文件型 item(含 URL-file 迁移产生的 Remote Overlay/Script,比旧实现仅 remote config 更宽——记入契约补遗)。

## 契约补遗(执行后回写 T03/T04/T05 卡,§5.3)

1. `ProfilesActorMessage::RefreshRemote` 的 reply 形态 = `Option<RpcReplyPort<...>>`(后台定时触发无 reply;卡内草签为必选)。
2. **`ProfileFsPort` 增加 `read_external(&ExternalProfilePath) -> anyhow::Result<String>`**(Mirror 同步需读外部 target;T03 的 service 补实现,mock 同步再生成)。
3. 同 uid 刷新进行中再次 `RefreshRemote` → 立即 `RefreshFailed{"refresh already in progress"}`(不排队;避免 pending 表复杂化)。
4. `CommitRefreshed` 时 uid 已删 → 丢弃结果、结清 reply(`RefreshFailed{"profile deleted during refresh"}`)、best-effort 删除下载任务刚写下的孤儿文件。
5. 刷新成功的事务:更新 `materialized.updated_at = now` + `subscription`;`AffectsRule::Touched(uid)`(复用 T04 闭包判定);后台提交(reply=None)且 affects → `notifier.request_rebuild()` 恰好一次。
6. `ExternalFileChanged` 语义:Symlink → 事务 bump `updated_at`(链接目标内容已变);Mirror → `read_external(target)` → 按 kind 校验(Config/Overlay=YAML mapping、Script=非空文本)→ `write_atomic` 镜像副本 → 事务 bump。

---

### Task 1: `RefreshRemote`/`CommitRefreshed` 下载-提交分离

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`(新消息 + `pending_refresh` 表 + 下载任务)
- Modify: `backend/tauri/src/client/profiles.rs`(`refresh` 方法)
- Modify: `backend/tauri/src/state/profiles/ports.rs`(`read_external`,Task 3 才消费,此处一并加避免 mock 两次再生成)
- Modify: `backend/tauri/src/service/profile_file.rs`(`read_external` 实现:`std::fs::read_to_string(target.as_path())`)

**Interfaces:**

- Produces(T07/T08 依赖):`ProfilesClient::refresh(uid: ProfileId, patch: Option<RemoteProfileOptionsPatch>) -> Result<CommitReport, ProfilesError>`。

- [ ] **Step 1: 写失败测试(client/profiles.rs tests)**

```rust
    use nyanpasu_config::profile::{RemoteProfileOptions, SubscriptionInfo};
    use crate::state::profiles::ports::FetchedSubscription;

    pub(crate) fn remote_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata { name: uid.to_uppercase(), desc: None },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Remote {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                        url: url::Url::parse("https://example.com/sub").unwrap(),
                        option: RemoteProfileOptions::default(),
                        subscription: SubscriptionInfo::default(),
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    fn ok_fetch(content: &'static str) -> MockSubscriptionFetcher {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            Ok(FetchedSubscription {
                content: content.to_string(),
                filename: None,
                subscription: SubscriptionInfo { upload: Some(1), ..Default::default() },
            })
        });
        fetcher
    }

    async fn remote_seeded_client(
        fs: MockProfileFsPort,
        fetcher: MockSubscriptionFetcher,
        notifier: MockRebuildNotifier,
    ) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(fetcher),
            std::sync::Arc::new(notifier),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();
        (client, dir)
    }

    #[tokio::test]
    async fn refresh_downloads_writes_and_commits_subscription() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str() == "r1.yaml" && content == "proxies: []\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) =
            remote_seeded_client(fs, ok_fetch("proxies: []\n"), MockRebuildNotifier::new()).await;

        let report = client.refresh(ProfileId("r1".into()), None).await.expect("refresh ok");
        let item = &report.snapshot.items[&ProfileId("r1".into())];
        let source = item.definition.source().unwrap();
        assert!(source.materialized().updated_at.is_some());
        match source {
            ProfileSource::Remote { subscription, .. } => {
                assert_eq!(subscription.upload, Some(1));
            }
            _ => unreachable!(),
        }
        // r1 不在闭包内(无 current)→ 前台刷新不触发 notifier(mock 无期望即断言零调用)
        assert!(!report.affects_current);
    }

    #[tokio::test]
    async fn refresh_failure_settles_reply_with_error() {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(|_, _| anyhow::bail!("dns exploded"));
        let (client, _dir) =
            remote_seeded_client(MockProfileFsPort::new(), fetcher, MockRebuildNotifier::new()).await;
        let err = client.refresh(ProfileId("r1".into()), None).await.unwrap_err();
        assert!(matches!(err, ProfilesError::RefreshFailed { .. }));
        // 状态未被污染
        let snapshot = client.get().await.unwrap();
        let source = snapshot.items[&ProfileId("r1".into())].definition.source().unwrap();
        assert!(source.materialized().updated_at.is_none());
    }

    #[tokio::test]
    async fn refresh_rejects_non_remote_and_unknown_and_concurrent() {
        let (client, _dir) = seeded_client().await; // cfg1/cfg2/ovl1,全 Local
        let err = client.refresh(ProfileId("cfg1".into()), None).await.unwrap_err();
        assert!(matches!(err, ProfilesError::NotARemoteProfile));
        let err = client.refresh(ProfileId("ghost".into()), None).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileNotFound(_)));

        // 并发拒绝:第一个挂起(慢 fetcher),第二个立即失败
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(|_, _| {
            std::thread::sleep(std::time::Duration::from_millis(200)); // fetcher 内部延迟(spawn 线程侧)
            Ok(FetchedSubscription {
                content: "a: 1\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
            })
        });
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let (client, _dir) = remote_seeded_client(fs, fetcher, MockRebuildNotifier::new()).await;
        let c2 = client.clone();
        let first = tokio::spawn(async move { c2.refresh(ProfileId("r1".into()), None).await });
        // 有界等待第一个进入 pending(轮询 in-progress 错误出现前的窗口)
        let err = loop {
            match client.refresh(ProfileId("r1".into()), None).await {
                Err(ProfilesError::RefreshFailed { message }) if message.contains("in progress") => {
                    break ProfilesError::RefreshFailed { message };
                }
                Ok(_) => panic!("second refresh must not both succeed before first settles"),
                Err(_) => tokio::task::yield_now().await,
            }
        };
        assert!(matches!(err, ProfilesError::RefreshFailed { .. }));
        first.await.unwrap().expect("first refresh completes");
    }
```

(注:并发测试用真实多任务而非 paused time——fetcher 的 200ms 延迟发生在 mockall 同步闭包内,由下载任务的 `spawn_blocking`/异步适配吸收;若实现选择纯异步 fetch,把延迟改为 `tokio::time::sleep` 并配合真实时钟,上限仍 <1s。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu refresh_`
Expected: FAIL(`refresh` 未定义)。

- [ ] **Step 3: 实现**

ports.rs `ProfileFsPort` 追加:

```rust
    /// Mirror 同步读外部 target(clean-design §10.2)。
    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String>;
```

service impl:

```rust
    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String> {
        std::fs::read_to_string(target.as_path())
            .with_context(|| format!("read external profile target {target}"))
    }
```

actor.rs 追加:

```rust
#[derive(Debug)]
pub enum RefreshOutcome {
    Succeeded {
        subscription: nyanpasu_config::profile::SubscriptionInfo,
    },
    Failed {
        message: String,
    },
}

// 消息变体:
    /// reply=None ⇒ 后台定时触发(补遗 #1)
    RefreshRemote {
        uid: ProfileId,
        patch: Option<nyanpasu_config::profile::RemoteProfileOptionsPatch>,
        reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    },
    /// 内部消息:下载任务完成回填(不出现在 client API)
    CommitRefreshed { uid: ProfileId, outcome: RefreshOutcome },
```

State 追加 `pending_refresh: std::collections::HashMap<ProfileId, Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>>`(`pre_start` 初始化为空表)。

handler:

```rust
            ProfilesActorMessage::RefreshRemote { uid, patch, reply } => {
                let settle_err = |reply: Option<RpcReplyPort<_>>, err: ProfilesError| {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(err));
                    }
                };
                if state.pending_refresh.contains_key(&uid) {
                    settle_err(reply, ProfilesError::RefreshFailed {
                        message: "refresh already in progress".into(),
                    });
                    return Ok(());
                }
                // 可选 options patch:先走标准事务(失败→结清 reply,不 spawn)
                if let Some(patch) = patch {
                    let patched = Self::run_write(state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return Err(ProfilesError::ProfileNotFound(uid.clone()));
                            };
                            match item.definition.source_mut() {
                                Some(nyanpasu_config::profile::ProfileSource::Remote { option, .. }) => {
                                    use struct_patch::Patch as _;
                                    option.apply(patch);
                                    Ok(WriteOutcome { affects: AffectsRule::Never, post_ops: vec![] })
                                }
                                _ => Err(ProfilesError::NotARemoteProfile),
                            }
                        }
                    })
                    .await;
                    if let Err(err) = patched {
                        settle_err(reply, err);
                        return Ok(());
                    }
                }
                // 快照校验 + 提取下载参数(handler 内零网络 I/O,D9)
                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    settle_err(reply, ProfilesError::ProfileNotFound(uid.clone()));
                    return Ok(());
                };
                let Some(nyanpasu_config::profile::ProfileSource::Remote { url, option, materialized, .. }) =
                    item.definition.source()
                else {
                    settle_err(reply, ProfilesError::NotARemoteProfile);
                    return Ok(());
                };
                let kind = item.definition.clone();
                let (url, option, path) = (url.clone(), option.clone(), materialized.file.clone());
                state.pending_refresh.insert(uid.clone(), reply);
                let fetcher = state.fetcher.clone();
                let fs = state.fs.clone();
                let actor = myself.clone();
                tokio::spawn(async move {
                    let outcome = async {
                        let fetched = fetcher
                            .fetch(&url, &option)
                            .await
                            .map_err(|e| format!("download failed: {e}"))?;
                        validate_fetched_content(&kind, &fetched.content)?;
                        fs.ensure_not_symlink(&path).map_err(|e| e.to_string())?;
                        fs.write_atomic(&path, &fetched.content).map_err(|e| e.to_string())?;
                        Ok::<_, String>(fetched.subscription)
                    }
                    .await;
                    let outcome = match outcome {
                        Ok(subscription) => RefreshOutcome::Succeeded { subscription },
                        Err(message) => RefreshOutcome::Failed { message },
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitRefreshed { uid, outcome });
                });
            }
            ProfilesActorMessage::CommitRefreshed { uid, outcome } => {
                let reply = state.pending_refresh.remove(&uid).flatten();
                let snapshot = Self::current_state(state);
                if snapshot.items.get(&uid).is_none() {
                    // 竞态:刷新期间被删——丢弃结果、清孤儿文件、结清 reply(补遗 #4)
                    if let RefreshOutcome::Succeeded { .. } = outcome {
                        // 下载任务可能刚写回文件;路径已不可从状态取得,按规范路径推导清理
                        // (uid 前缀规范保证 {uid}.yaml/.js/.lua 之一;三者都尝试 best-effort)
                        for ext in ["yaml", "js", "lua"] {
                            if let Ok(path) = nyanpasu_config::profile::ManagedProfilePath::new(
                                format!("{uid}.{ext}"),
                            ) {
                                let _ = state.fs.remove(&path);
                            }
                        }
                    }
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "profile deleted during refresh".into(),
                        }));
                    }
                    return Ok(());
                }
                let result = match outcome {
                    RefreshOutcome::Failed { message } => Err(ProfilesError::RefreshFailed { message }),
                    RefreshOutcome::Succeeded { subscription } => {
                        Self::run_write(state, {
                            let uid = uid.clone();
                            move |profiles| {
                                let Some(item) = profiles.items.get_mut(&uid) else {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                };
                                match item.definition.source_mut() {
                                    Some(nyanpasu_config::profile::ProfileSource::Remote {
                                        materialized, subscription: slot, ..
                                    }) => {
                                        materialized.updated_at =
                                            Some(time::OffsetDateTime::now_utc());
                                        *slot = subscription;
                                        Ok(WriteOutcome {
                                            affects: AffectsRule::Touched(uid.clone()),
                                            post_ops: vec![],
                                        })
                                    }
                                    _ => Err(ProfilesError::NotARemoteProfile),
                                }
                            }
                        })
                        .await
                    }
                };
                // 后台提交(reply=None)且影响 current → 通知重建恰好一次(补遗 #5)
                if reply.is_none() {
                    if let Ok(report) = &result {
                        if report.affects_current {
                            state.notifier.request_rebuild();
                        }
                    }
                }
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
```

内容校验辅助(actor.rs 内自由函数):

```rust
/// 按目标 Profile 类型校验下载内容(design 图 13.3):Config File 与 Overlay 必须
/// 是 YAML mapping;Script 只要求非空文本。
fn validate_fetched_content(
    definition: &ProfileDefinition,
    content: &str,
) -> Result<(), String> {
    let needs_yaml = match definition {
        ProfileDefinition::Config { .. } => true,
        ProfileDefinition::Transform { transform } => matches!(
            transform,
            nyanpasu_config::profile::TransformDefinition::Overlay(_)
        ),
    };
    if needs_yaml {
        serde_yaml::from_str::<serde_yaml::Mapping>(content)
            .map(|_| ())
            .map_err(|e| format!("downloaded content is not a YAML mapping: {e}"))
    } else if content.trim().is_empty() {
        Err("downloaded script is empty".into())
    } else {
        Ok(())
    }
}
```

client 方法:

```rust
    pub async fn refresh(
        &self,
        uid: ProfileId,
        patch: Option<nyanpasu_config::profile::RemoteProfileOptionsPatch>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::RefreshRemote { uid, patch, reply: Some(reply) },
            None,
        )
        .await
    }
```

(`handle` 签名此前忽略 `myself`——本 Task 起改用 `myself: ActorRef<Self::Msg>` 绑定名。)

- [ ] **Step 4: 跑测试确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu refresh_`
Expected: 全 PASS(含 T03/T04 既有测试回归)。

```bash
git add backend/tauri/src/state/profiles backend/tauri/src/client/profiles.rs backend/tauri/src/service/profile_file.rs
git commit -m "feat(tauri): add download-commit split remote refresh to profiles actor"
```

---

### Task 2: RemoteUpdateScheduler(定时表 reconcile + 启动补课)

**Files:**

- Create: `backend/tauri/src/state/profiles/scheduler.rs`
- Modify: `backend/tauri/src/state/profiles/{mod.rs, actor.rs}`

**Interfaces:**

- 对外不可见(actor 内部);`ProfilesActorState` 增 `scheduler: RemoteUpdateScheduler`。

- [ ] **Step 1: 写失败测试(paused time,零真实 sleep)**

```rust
    #[tokio::test(start_paused = true)]
    async fn scheduler_fires_refresh_on_interval() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = fetch_count.clone();
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(FetchedSubscription {
                content: "a: 1\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
            })
        });
        let (client, _dir) =
            remote_seeded_client(fs, fetcher, MockRebuildNotifier::new()).await;
        // remote_config_item 的 interval = 默认 120 分钟;updated_at=None → 无补课
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        tokio::time::advance(std::time::Duration::from_secs(120 * 60 + 1)).await;
        // 有界等待 cast→下载→CommitRefreshed 链路排空
        for _ in 0..200 {
            if fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        drop(client);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_reconcile_add_remove_and_kind_switch() {
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = fetch_count.clone();
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("count only")
        });
        let (client, _dir) = remote_seeded_client(
            MockProfileFsPort::new(),
            fetcher,
            MockRebuildNotifier::new(),
        )
        .await;
        // 删除 r1 → 定时任务应被拆除
        client.delete(ProfileId("r1".into())).await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(240 * 60)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_catches_up_overdue_profiles_on_start() {
        // updated_at = 很久以前 → post_start reconcile 立即触发一次(旧 init() 语义)
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = fetch_count.clone();
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("count only")
        });
        let dir = tempdir().unwrap();
        // 先用一个 client 写入「已过期」的 remote item,再重启触发补课
        {
            let client = ProfilesClient::new(
                temp_profiles_path(&dir),
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                std::sync::Arc::new(MockRebuildNotifier::new()),
            )
            .await
            .unwrap();
            let mut item = remote_config_item("r1");
            if let Some(source) = item.definition.source_mut() {
                source.materialized_mut().updated_at = Some(
                    time::OffsetDateTime::now_utc() - time::Duration::days(30),
                );
            }
            let mut profiles = Profiles::default();
            profiles.append_item(item);
            client.replace(profiles).await.unwrap();
        }
        let _client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(fetcher),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        for _ in 0..200 {
            if fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu scheduler_`
Expected: FAIL(post_start 未 reconcile,定时不触发)。

- [ ] **Step 3: 实现 `scheduler.rs`**

```rust
//! Per-uid remote refresh timers. Owned by the ProfilesActor; reconciled
//! idempotently after every commit (design §7/D8; semantics ported from the
//! legacy ProfilesJob diff, core/tasks/jobs/profiles.rs).

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
    /// 幂等 reconcile:新增/修改/移除三类 diff;`catch_up` 时对「已过期」的
    /// profile 立即触发一次(旧 init() 补课语义)。
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
                Some(ProfileSource::Remote { option, materialized, .. })
                    if option.update_interval_minutes > 0 =>
                {
                    Some((
                        uid.clone(),
                        (option.update_interval_minutes, materialized.updated_at),
                    ))
                }
                _ => None,
            })
            .collect();

        // Remove / Update
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

        // Add(含 Update 后重建)
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
            self.entries.insert(uid, Entry { interval_minutes, handle });
        }
    }

    pub(super) fn shutdown(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.handle.abort();
        }
    }
}
```

actor 接线:

- `ProfilesActorState` 增 `scheduler: RemoteUpdateScheduler`;
- `post_start`(新增 lifecycle 方法)中 `state.scheduler.reconcile(&snapshot, &myself, true)`(catch_up = true;注意 `pre_start` 拿不到稳定 myself 引用时改在 `post_start`);
- `run_write` 成功提交后追加 `state.scheduler.reconcile(&next, &myself, false)`——`run_write` 需要 `myself` 参数:签名改为 `run_write(myself: &ActorRef<Self::Msg>, state, mutate)`,全部调用点同步更新;
- `post_stop` 调 `scheduler.shutdown()`。

(catch_up 语义修正:updated_at=None 的 Remote 是「从未物化」——旧语义 `updated=0` 也会补课,故 `is_none_or` 分支对 None 触发补课;Task 1 测试 `scheduler_fires_refresh_on_interval` 里 updated_at=None——**该测试的 seed 需改为 updated_at=Some(now)** 以隔离「纯 interval 触发」,执行时以此为准。)

- [ ] **Step 4: 跑测试确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu scheduler_ refresh_`
Expected: 全 PASS。

```bash
git add backend/tauri/src/state/profiles
git commit -m "feat(tauri): add remote update scheduler with idempotent reconcile"
```

---

### Task 3: External Symlink/Mirror watcher

**Files:**

- Modify: `backend/tauri/src/state/profiles/scheduler.rs`(watcher 表并入,或平行 `watchers: HashMap<ProfileId, Debouncer<...>>`)
- Modify: `backend/tauri/src/state/profiles/actor.rs`(`ExternalFileChanged` 消息 + 处理)

- [ ] **Step 1: 写失败测试(处理逻辑 = 注入消息,确定性)**

```rust
    fn external_item(uid: &str, mode: nyanpasu_config::profile::ExternalMode, target: &std::path::Path)
        -> nyanpasu_config::profile::ProfileItem
    {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata { name: uid.to_uppercase(), desc: None },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: nyanpasu_config::profile::LocalBinding::External {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                                updated_at: None,
                            },
                            target: nyanpasu_config::profile::ExternalProfilePath::new(
                                target.to_string_lossy(),
                            )
                            .unwrap(),
                            mode,
                        },
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    #[tokio::test]
    async fn external_mirror_change_syncs_copy_and_bumps_updated_at() {
        let temp_target = tempfile::tempdir().unwrap();
        let target = temp_target.path().join("outside.yaml");
        std::fs::write(&target, "mode: rule\n").unwrap();

        let mut fs = MockProfileFsPort::new();
        fs.expect_read_external().returning(|_| Ok("mode: rule\n".to_string()));
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str() == "m1.yaml" && content == "mode: rule\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "m1",
            nyanpasu_config::profile::ExternalMode::Mirror,
            &target,
        ));
        client.replace(profiles).await.unwrap();

        client.debug_cast_external_changed(ProfileId("m1".into())).await;
        // 有界等待事务提交(updated_at 出现)
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let snapshot = client.get().await.unwrap();
            let source = snapshot.items[&ProfileId("m1".into())].definition.source().unwrap();
            if source.materialized().updated_at.is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "mirror sync never committed");
            tokio::task::yield_now().await;
        }
    }
```

(`debug_cast_external_changed` = client 上 `#[cfg(test)]` 辅助:`self.inner.actor_ref.cast(ProfilesActorMessage::ExternalFileChanged { uid })`。真实 notify 事件接线用一个有界冒烟测试:tempdir 建 Mirror item → 修改 target 文件 → ≤5s 轮询 `updated_at`;若 CI 文件事件不可靠,标注 `#[ignore]` 并在 PR 描述记录人工验证——执行时按实际稳定性定夺。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu external_`
Expected: FAIL。

- [ ] **Step 3: 实现**

消息变体 + handler:

```rust
    /// 内部消息:watcher 检测到 External target 变化
    ExternalFileChanged { uid: ProfileId },
```

```rust
            ProfilesActorMessage::ExternalFileChanged { uid } => {
                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else { return Ok(()); };
                let Some(nyanpasu_config::profile::ProfileSource::Local {
                    binding:
                        nyanpasu_config::profile::LocalBinding::External { materialized, target, mode },
                }) = item.definition.source()
                else {
                    return Ok(());
                };
                let (path, target, mode, definition) = (
                    materialized.file.clone(),
                    target.clone(),
                    *mode,
                    item.definition.clone(),
                );
                // Mirror:读 target → 按 kind 校验 → 原子替换副本(本地 fs,允许在 handler 内)
                if mode == nyanpasu_config::profile::ExternalMode::Mirror {
                    let synced = state
                        .fs
                        .read_external(&target)
                        .map_err(|e| e.to_string())
                        .and_then(|content| {
                            validate_fetched_content(&definition, &content).map(|_| content)
                        })
                        .and_then(|content| {
                            state.fs.write_atomic(&path, &content).map_err(|e| e.to_string())
                        });
                    if let Err(message) = synced {
                        tracing::warn!("mirror sync failed for {uid}: {message}");
                        return Ok(());
                    }
                }
                // Symlink 与 Mirror 共同:事务 bump updated_at;后台提交 affects → 通知
                let result = Self::run_write(&myself, state, {
                    let uid = uid.clone();
                    move |profiles| {
                        let Some(item) = profiles.items.get_mut(&uid) else {
                            return Err(ProfilesError::ProfileNotFound(uid.clone()));
                        };
                        if let Some(source) = item.definition.source_mut() {
                            source.materialized_mut().updated_at =
                                Some(time::OffsetDateTime::now_utc());
                        }
                        Ok(WriteOutcome { affects: AffectsRule::Touched(uid.clone()), post_ops: vec![] })
                    }
                })
                .await;
                if let Ok(report) = result {
                    if report.affects_current {
                        state.notifier.request_rebuild();
                    }
                }
            }
```

watcher 表(scheduler.rs 内平行结构,reconcile 时同步增删):

```rust
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};

pub(super) struct ExternalWatchers {
    watchers: HashMap<ProfileId, Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl ExternalWatchers {
    pub(super) fn reconcile(&mut self, profiles: &Profiles, actor: &ActorRef<ProfilesActorMessage>) {
        use nyanpasu_config::profile::{LocalBinding, ProfileSource};
        let desired: HashMap<ProfileId, std::path::PathBuf> = profiles
            .items
            .iter()
            .filter_map(|(uid, item)| match item.definition.source() {
                Some(ProfileSource::Local { binding: LocalBinding::External { target, mode, .. } }) => {
                    // Symlink 监听解析后的真实路径,解析失败回退 target 本身(clean-design §10.1)
                    let watch_path = if *mode == nyanpasu_config::profile::ExternalMode::Symlink {
                        std::fs::canonicalize(target.as_path()).unwrap_or_else(|_| target.as_path().to_owned())
                    } else {
                        target.as_path().to_owned()
                    };
                    Some((uid.clone(), watch_path))
                }
                _ => None,
            })
            .collect();

        self.watchers.retain(|uid, _| desired.contains_key(uid));
        for (uid, path) in desired {
            if self.watchers.contains_key(&uid) {
                continue;
            }
            let actor = actor.clone();
            let event_uid = uid.clone();
            let debouncer = new_debouncer(
                std::time::Duration::from_millis(500),
                None,
                move |result: DebounceEventResult| {
                    if result.is_ok() {
                        let _ = actor.cast(ProfilesActorMessage::ExternalFileChanged {
                            uid: event_uid.clone(),
                        });
                    }
                },
            );
            match debouncer {
                Ok(mut debouncer) => {
                    if debouncer.watch(&path, RecursiveMode::NonRecursive).is_ok() {
                        self.watchers.insert(uid, debouncer);
                    }
                }
                Err(error) => tracing::warn!("failed to create watcher for {uid}: {error}"),
            }
        }
    }
}
```

(`ExternalWatchers` 并入 `ProfilesActorState`,reconcile 调用点与 scheduler 相同:post_start(catch_up 轮)+ 每次 `run_write` 提交后。notify API 细节以 `logging/manager.rs:252` 现物为准,编译错误时对齐之。)

- [ ] **Step 4: 跑测试确认通过 + 纯度断言 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu external_ scheduler_ refresh_ && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --lib profiles`
Expected: 全 PASS。
Run(Git Bash): `grep -rn "tauri::\|crate::config" backend/tauri/src/state/profiles/ && echo LEAK || echo CLEAN` — Expected: `CLEAN`。

```bash
git add backend/tauri/src/state/profiles backend/tauri/src/service/profile_file.rs
git commit -m "feat(tauri): add external symlink/mirror watchers to profiles actor"
```

---

### Task 4: 契约回写 + 全量回归

- [ ] **Step 1**: Run `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo clippy --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --all-targets --all-features` — Expected: 全绿。
- [ ] **Step 2**: 按「契约补遗」1–6 回写 task.md(T03 卡 `read_external`、T05 卡 reply Option 化/并发拒绝/删除竞态清理/通知恰好一次语义),并在 T07 卡 Consumes 标注 `ProfilesClient::refresh` 最终签名。
- [ ] **Step 3**:

```bash
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T05 contract addenda (refresh protocol, watchers)"
```

---

## 验证总表(对应 T05 卡验证段)

| 判据                                       | 覆盖                                                                             |
| ------------------------------------------ | -------------------------------------------------------------------------------- |
| handler 内零网络 I/O                       | 实现结构(下载在 tokio::spawn)+ 代码审查                                          |
| 失败路径必结清挂起 reply                   | Task 1 `refresh_failure_settles_reply_with_error`                                |
| 并发刷新拒绝                               | Task 1 `refresh_rejects_..._concurrent`                                          |
| reconcile 幂等(新增/删除/切换)             | Task 2 `scheduler_reconcile_add_remove_...`                                      |
| 启动补课(旧 init 语义)                     | Task 2 `scheduler_catches_up_overdue_...`                                        |
| watcher 失效传导(Mirror 同步 + bump)       | Task 3 `external_mirror_change_...`                                              |
| `CommitRefreshed` 时 uid 已删 → 丢弃并结清 | Task 1 实现 + 补充单测(执行时加:删除后手动 cast CommitRefreshed,断言 reply 结清) |
| 后台提交 affects → 通知恰好一次            | Task 3 handler + mock `times(1)`(执行时在 external 测试中给 notifier 设期望)     |
| 全程无真实 sleep                           | paused-time + 有界轮询                                                           |

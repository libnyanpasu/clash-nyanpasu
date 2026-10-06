# PR-3 T08 — IPC 全量 BC 切换(13 → 16 条)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ipc.rs` 全部 13 条 profile 命令重写为 thin adapter(解析 DTO → `NyanpasuClient` → 域错误映射),`patch_profiles_config`/`patch_profile` 拆除、5 条新命令加入(净 16 条);specta 注册表同步。**自本卡起前端类型检查红,直至 T09**(§4 铁律 3 预期形态)。

**Architecture:** 命令层零业务编排——唯一的多步域流程(import:add → 首次下载 refresh → 条件 activate)收编为 **facade 复合方法 `import_profile`**(编排属 facade,design §6.4);连接中断/UI 刷新已在 T07 的 `rebuild_running_config` 统一收口,命令不再散调 `CoreManager::global()`/`Handle`。每条命令形如:

```rust
#[tauri::command]
#[specta::specta]
pub async fn delete_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    client.delete_profile(uid).await?;
    Ok(())
}
```

**Tech Stack:** T07 facade 全部方法、`IpcError::Profiles`(T07 Task 2)、T01 specta 域类型(`Profiles`/`ProfileId`/`NewProfileRequest`/`ProfileDefinition`/`ProfileMetadataPatch`/`RemoteProfileOptionsPatch` 均 derive `specta::Type`)。

## Global Constraints

- 一切 cargo 操作前设 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`。
- 每 commit `cargo build` + `cargo test` 绿;**前端红是预期**,不看前端。
- 命令体判据(CLAUDE.md §12):每条 = DTO 解析 → 一次 facade 调用(import 亦是一次) → `?` 错误映射;`grep "Config::profiles()" ipc.rs` 零命中。
- T06A golden 套件保持全绿。

## 基线事实(2026-07-06 实测)

- 现 13 条命令与行锚:`get_profiles:103`(TODO 桥注释在体内,随重写消失)、`enhance_profiles:132`(update_config+refresh_clash)、`import_profile:140`(RemoteProfileBuilder 下载→append→条件 `patch_profiles_config_inner`)、`create_profile:175`(ProfileBuilder 四变体)、`reorder_profile:238`、`reorder_profiles_by_list:246`、`update_profile:254`(→feat::update_profile)、`delete_profile:261`、`patch_profiles_config:284` + `patch_profiles_config_inner:288-310`(唯一 `ConnectionInterruptionService` 调用点,T07 已收口到 rebuild)、`patch_profile:315`、`view_profile:361`、`read_profile_file:381`、`save_profile_file:398`。
- specta 注册块:`specta_export.rs:53-65`(`// profile` 组);`lib.rs` 仅消费 builder(199/238/249)。
- 错误面:`IpcError::Profiles(ProfilesError)` + `From<ClientError>` 臂已就绪(T07 Task 2);IpcError 序列化为 debug 字符串(ipc.rs:69-76),前端 toast 直接可读。
- **import 语义差**:legacy 在 builder 内同步下载并按 content-disposition 命名;新域 actor 的 Add 不触发下载(无 PostOp 首刷机制,scheduler 仅按 interval),`RefreshRemote` 不回写名称(actor.rs 无 filename 消费)→ import = add(占位名) → refresh(首次物化) → 条件 activate;**命名 BC**:显式 fallback(url 末段/host),用户可改名(T09 编辑对话框)。
- `ProfileSource::Remote { materialized(flatten), url: Url, option: RemoteProfileOptions(#[serde(default)]), subscription }`(source.rs:20-);`RemoteProfileOptions` 带 struct_patch(`RemoteProfileOptions::default().apply(patch)` 得全量)。
- 条件激活判据:add 的 `CommitReport.snapshot.current.is_none()`(add 不改 current,报告即 add 后现场)。
- bindings.ts 自动导出仅发生在 **debug run**(lib.rs:201-224,`run()` 前段,先于 Builder——BC 态启动后续崩溃不影响导出);本卡只改 Rust 面,导出留给 T09 首步。

## 契约修正(执行后回写 task.md T08 卡 + T07 卡 Produces 表,§5.3)

1. **facade 增 `import_profile(url: Url, options: Option<RemoteProfileOptionsPatch>) -> Result<ProfileId>`**(T07 Produces 表补一行):add(Remote 定义,占位名)→ `refresh_profile`(首次下载物化;失败 = import 失败,已提交的空壳 profile 回删)→ `snapshot.current == None` 则 `activate_profile`。命令层保持单调用。
2. import 命名 BC:fallback = url path 末段(去扩展名)或 host;content-disposition 命名能力随 legacy builder 退役(记录为已接受 BC,T09 提供改名交互)。
3. 测试面:命令均为单行 thin,不引 `tauri::test`;行为断言收敛在 facade 测试(T07 + 本卡 import 测试)与 `IpcError` 映射;命令层验证 = 编译 + grep 判据。
4. `Generated: bindings.ts` 从本卡移出:导出动作发生于 T09 首步(debug run),本卡后前端红为预期。
5. `enhance_profiles` 不再显式 `refresh_clash`(rebuild 内 UiEventSink 已发同事件,避免双发)。

---

### Task 1: facade `import_profile` 复合方法

**Files:**

- Modify: `backend/tauri/src/client/mod.rs`(方法 + 测试;测试需注入 mock fetcher 的构造 helper)

**Interfaces:**

- Consumes: T07 `add_profile`/`refresh_profile`/`activate_profile`/`delete_profile`、`ProfilesClient` 测试注入先例(client/profiles.rs tests 的 MockSubscriptionFetcher)。
- Produces: `pub async fn import_profile(&self, url: url::Url, options: Option<RemoteProfileOptionsPatch>) -> Result<ProfileId>`(T08 Task 2 消费)。

- [ ] **Step 1: 测试构造 helper**——client/mod.rs tests 增(绕开真实 HTTP):

```rust
    /// Build a facade whose profiles domain uses injected fs/fetcher mocks.
    async fn test_client_with_profiles_ports(
        dir: &TempDir,
        fs: Arc<dyn crate::state::profiles::ports::ProfileFsPort>,
        fetcher: Arc<dyn crate::state::profiles::ports::SubscriptionFetcher>,
        core: Arc<dyn RunningCoreBridge>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let ports = Arc::new(SessionPortResolver::default());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            fs.clone(),
            fetcher,
            Arc::new(rebuild::ChannelRebuildNotifier::new(tx)),
        )
        .await
        .expect("profiles client");
        let client = NyanpasuClient::with_parts(
            application, session_state, clash_config, profiles, fs, ports,
            dir.path().join("profiles"),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core,
        );
        let listener = client.clone();
        rebuild::spawn_listener_with(rx, move || {
            let client = listener.clone();
            async move { client.rebuild_running_config().await.map_err(anyhow::Error::from) }
        });
        client
    }
```

(mock 类型直接复用 client/profiles.rs 测试模块的 `MockProfileFsPort`/`MockSubscriptionFetcher` 构造方式——若其为模块私有,将 mockall 生成物经 `pub(crate)` re-export 或在本测试中重新 `mock!` 声明,以现场最小改动为准。)

- [ ] **Step 2: 写失败测试**:

```rust
    #[test]
    fn facade_import_downloads_and_conditionally_activates() {
        let dir = tempdir().unwrap();
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .times(1)
            .returning(|_, _| {
                Ok(crate::state::profiles::ports::FetchedSubscription {
                    content: "proxies: []\n".into(),
                    subscription: None,
                    filename: Some("sub.yaml".into()),
                })
            });
        let mut core = MockRunningCoreBridge::new();
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_profiles_ports(
                &dir, Arc::new(fs), Arc::new(fetcher), Arc::new(core),
            )
            .await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let uid = client.import_profile(url, None).await.expect("import");
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(snapshot.current.as_ref(), Some(&uid), "empty current must auto-activate");
            let item = &snapshot.items[&uid];
            assert_eq!(item.metadata.name, "my-sub"); // url 末段 fallback 命名
            assert!(item.definition.source().unwrap().is_remote());
        });
    }
```

(`FetchedSubscription` 字段名以 `state/profiles/ports.rs` 实物为准(T03/T05 交付);mock 期望方法集按 `RefreshRemote` 下载任务实际调用的 fs/fetcher 方法微调——以首次运行的 mockall panic 消息为准补齐,不改断言。)

- [ ] **Step 3: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu facade_import_downloads`
Expected: 编译错(`import_profile` 未定义)。

- [ ] **Step 4: 实现**(client/mod.rs):

```rust
    /// Import a remote subscription: add (placeholder name) → first download
    /// via the refresh transaction → auto-activate when nothing is current.
    /// Naming BC (recorded 2026-07-06): legacy content-disposition naming is
    /// retired; the fallback is the url's last path segment (sans extension)
    /// or the host.
    pub async fn import_profile(
        &self,
        url: url::Url,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> Result<ProfileId> {
        let name = url
            .path_segments()
            .and_then(|segments| segments.filter(|s| !s.is_empty()).next_back())
            .map(|segment| segment.trim_end_matches(".yaml").trim_end_matches(".yml").to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| url.host_str().map(str::to_string))
            .unwrap_or_else(|| "Remote Profile".into());
        let mut option = RemoteProfileOptions::default();
        if let Some(patch) = options {
            option.apply(patch);
        }
        let request = NewProfileRequest {
            metadata: ProfileMetadata { name, desc: None },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Remote {
                        materialized: MaterializedFile {
                            // Add rewrites the path to `{uid}.{ext}` server-side.
                            file: ManagedProfilePath::new("pending.yaml")
                                .expect("static managed path is valid"),
                            updated_at: None,
                        },
                        url,
                        option,
                        subscription: None,
                    },
                    transforms: vec![],
                }),
            },
        };
        let report = self.inner.profiles.add(request, None).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("import committed without a created uid".into()))?;
        let was_empty = report.snapshot.current.is_none();
        if let Err(error) = self.refresh_profile(created.clone(), None).await {
            // 首次下载失败 = import 失败;回删空壳,保持 legacy 原子观感。
            let _ = self.inner.profiles.delete(created.clone()).await;
            return Err(error);
        }
        if was_empty {
            self.activate_profile(Some(created.clone())).await?;
        }
        Ok(created)
    }
```

(`ProfileSource::Remote` 字段名 `materialized/url/option/subscription` 以 source.rs:20-32 实物为准;`Patch::apply` 来自 `struct_patch::Patch`,import 处补 `use struct_patch::Patch as _;`。)

- [ ] **Step 5: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu facade_import`
Expected: PASS;全量 `cargo test -p clash-nyanpasu` 绿。

```powershell
git add backend/tauri/src/client/mod.rs
git commit -m "feat(tauri): add import_profile composite to client facade"
```

---

### Task 2: ipc.rs 命令面重写(13 → 16)

**Files:**

- Modify: `backend/tauri/src/ipc.rs`(:101-410 的 profile 命令区整体替换;头部 imports 修剪)

**Interfaces:**

- Consumes: facade 16 方法(T07 表 + Task 1 import);`IpcError::Profiles`。
- Produces: 16 条命令名(T09 依赖):`get_profiles / enhance_profiles / import_profile / create_profile / reorder_profile / reorder_profiles_by_list / update_profile / delete_profile / activate_profile / set_global_transforms / patch_profile_metadata / patch_remote_profile_options / replace_profile_definition / view_profile / read_profile_file / save_profile_file`。

- [ ] **Step 1: 整区替换**——profile 命令区(:101-108 与 :130-410 中 13 条)替换为:

```rust
// ---- profiles domain commands (PR-3 T08, thin adapters over NyanpasuClient) ----

use crate::state::profiles::actor::NewProfileRequest;
use nyanpasu_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadataPatch, Profiles as DomainProfiles,
    RemoteProfileOptionsPatch,
};

#[tauri::command]
#[specta::specta]
pub async fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

#[tauri::command]
#[specta::specta]
pub async fn enhance_profiles(client: State<'_, NyanpasuClient>) -> Result {
    client.rebuild_running_config().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    client.import_profile(url, option).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result {
    let uid = client.add_profile(request, file_data).await?;
    // 自动激活条件 = Config 定义(含 Composition)且当前无激活(design §9)
    let snapshot = client.get_profiles().await?;
    let is_config = matches!(
        snapshot.items.get(&uid).map(|item| &item.definition),
        Some(ProfileDefinition::Config { .. })
    );
    if is_config && snapshot.current.is_none() {
        client.activate_profile(Some(uid)).await?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result {
    client.reorder_profile(active_id, over_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result {
    client.reorder_profiles_by_list(list).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result {
    client.refresh_profile(uid, option).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result {
    client.delete_profile(uid).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result {
    client.activate_profile(uid).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_global_transforms(
    client: State<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result {
    client.set_global_transforms(ids).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result {
    client.patch_profile_metadata(uid, patch).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result {
    client.patch_remote_profile_options(uid, patch).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result {
    client.replace_profile_definition(uid, definition).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn view_profile(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result {
    let path = client.get_profile_materialized_path(uid).await?;
    if !path.exists() {
        return Err(IpcError::Custom("profile file not found".into()));
    }
    help::open_file(app_handle, path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn read_profile_file(client: State<'_, NyanpasuClient>, uid: ProfileId) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result {
    client.save_profile_file(uid, file_data).await?;
    Ok(())
}
```

要点:

- `NewProfileRequest` 从 `crate::state::profiles::actor` 导入(域类型群从 `nyanpasu_config::profile`);顶部 use 块相应改写,**删除** `ProfileBuilder`/`ProfilesBuilder`/`RemoteProfileOptionsBuilder`/`crate::config::profile::item::RemoteProfileBuilder`/`ProfileItemType` 等仅被旧命令消费的 import(类型本体 T10 才删);`patch_profiles_config`/`patch_profiles_config_inner`/`patch_profile` 三个 fn 整体删除。
- `uid: ProfileId` 直接作为参数类型(`ProfileId(String)` derive Deserialize/Type,serde transparent 与否以 T01 实物为准;若前端序列化为裸 string 而 ProfileId 非 transparent,则参数收 `String` 边界转 `ProfileId`——以 bindings 导出后的 TS 形状定,记入 T09)。
- `save_profile_file` 参数从 `file_data: Option<String>` 收紧为 `String`(legacy 的 None 分支本就是无操作;BC 记录)。
- `NewProfileRequest` 若未 derive `specta::Type`/`Deserialize`(actor 内部类型),在 actor.rs 为其补 derive(仅注解,无行为)。

- [ ] **Step 2: specta 注册表**——specta_export.rs:53-65 替换:

```rust
            // profile
            ipc::get_profiles,
            ipc::enhance_profiles,
            ipc::import_profile,
            ipc::create_profile,
            ipc::reorder_profile,
            ipc::reorder_profiles_by_list,
            ipc::update_profile,
            ipc::delete_profile,
            ipc::activate_profile,
            ipc::set_global_transforms,
            ipc::patch_profile_metadata,
            ipc::patch_remote_profile_options,
            ipc::replace_profile_definition,
            ipc::view_profile,
            ipc::read_profile_file,
            ipc::save_profile_file,
```

- [ ] **Step 3: 验证**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 绿。
Run: `Select-String -Path backend/tauri/src/ipc.rs -Pattern "Config::profiles\(\)"`
Expected: 零命中。
Run: `Select-String -Path backend/tauri/src/ipc.rs -Pattern "patch_profiles_config|ProfilesBuilder"`
Expected: 零命中。

- [ ] **Step 4: Commit**

```powershell
git add backend/tauri/src/ipc.rs backend/tauri/src/specta_export.rs backend/tauri/src/state/profiles/actor.rs
git commit -m "feat(tauri)!: switch profile ipc surface to domain commands (13 to 16)"
```

---

### Task 3: 契约回写

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T08 卡尾部 + T07 卡 Produces 表)

- [ ] **Step 1: T07 卡 Produces 方法表补一行**

```rust
    pub async fn import_profile(&self, url: Url, options: Option<RemoteProfileOptionsPatch>) -> Result<ProfileId>; // 2026-07-06 T08:add→首刷→条件激活的复合编排
```

- [ ] **Step 2: T08 卡追加执行修正块**

```markdown
**2026-07-06 执行修正(T08 实物)**:

- import 编排收编 facade `import_profile`(add 占位名 → refresh 首次物化,失败回删 → current==None 激活);命名 BC:content-disposition 命名退役,fallback = url 末段/host。
- `create_profile` 请求 DTO = `NewProfileRequest`(uid 服务端生成,D13);自动激活在命令层按快照判定(Config 定义 + current==None)。
- `save_profile_file` 参数收紧为必填 `String`(legacy Option::None 分支为无操作)。
- `enhance_profiles` 不再显式 refresh_clash(rebuild 内 UiEventSink 同事件,避免双发)。
- bindings.ts 导出移交 T09 首步(debug run 触发)。
```

- [ ] **Step 3: Commit**

```powershell
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T08 execution addenda in task card"
```

---

## Self-Review 结论

- 覆盖:§9 表 13 行逐条落位;5 条新命令 + 2 条删除 = 16 条与注册表一致;`ConnectionInterruptionService` 唯一散调点随 inner 删除(T07 已收口)。
- 无占位符:两处「以实物为准」为 derive/transparent 注解适配(编译器强制显形),非 TBD。
- 类型一致性:命令签名逐一对应 facade 方法(Task 1 的 import 签名 = T07 表增行);`DomainProfiles` 别名避免与旧 `Profiles` import 冲突(旧 import 本卡移除后可直接用 `Profiles`)。

# PR-3(R-3) — profiles 域切换 任务拆解(task.md)

- **关联设计**: [`./design.md`](./design.md)(下文「design §N」均指该文件章节)
- **拆解目标**: 每个任务 = 一个可独立 plan、独立执行、独立过审的 commit 组(1–3 个 conventional commit);任务卡自带 scope、接口契约与验证判据,供后续用 `superpowers:writing-plans` 逐卡展开为 bite-sized plan(执行时配合 `superpowers:subagent-driven-development` 或 `executing-plans`)。
- **分支策略**: 单一 feature 分支(建议 `refactor/pr3-profiles-domain-switch`),任务按依赖序落 commit;T07–T10 为**原子切换组**(见 §4),必须同 PR 合并。
- **前置状态(2026-07-06 勘误)**: pre①(#4868)/pre②(#4877)/PR-2b(#4869)均已合并,基准 = main @ `356864d5`;executor 实物签名与 D12 落定见 design §19,T06/T07 卡已同步。

---

## 0. 全局约束(每张任务卡隐含,plan 时逐条带入)

1. CLAUDE.md 铁律:无新 `::global()` / 静态可变服务;依赖显式注入;Tauri 隔离在适配器后。
2. `state/profiles.rs` + `client/profiles.rs` **禁止** import `tauri::*` / `crate::config::Config`(design D10)。
3. RPC 超时:读 `call(_, Some(PROFILES_READ_TIMEOUT))`(5s 域内常量)、写 `call(_, None)`;写 handler 禁无界 I/O(design D9)。
4. 全部 mutation 走七步事务:clone→mutate→`validate()`→scheduler diff→原子持久化→commit→重建索引+reconcile;commit 后副作用失败 = 降级不回滚(design D5)。
5. 本 PR 允许的旧全局消费点仅:`Config::runtime()` 写产物、`CoreManager::global().update_config()` 两处(D12 已落定走 typed client,取数点不复存在——design §19)——全部标 `TODO(actor-migration)` 注释(格式见 design §8),台账 B8。
6. 测试不 sleep,同步用 `RpcReplyPort` ack;ports 兼容 `mockall::automock`。
7. 硬前置(2026-07-06 核对,**全部满足**):PR-3-pre①(snapshot store v2)已合并 `ffd80168`(#4868);PR-3-pre②(runtime pipeline executor)已合并 `356864d5`(#4877);PR-2b(三 StateActor)已合并 `95c4ca8a`(#4869)→ D12 走 typed client 分支(design §19)。T06 解除阻塞。
8. 中间态规则:每个 commit 必须 `cargo build` + `cargo test` 绿;「应用端到端可运行」只在原子切换组边界(T07 之前 / T10 之后)保证。

---

## 1. 任务依赖图

```mermaid
flowchart LR
  subgraph pre["硬前置(非本 PR)"]
    P2["PR-3-pre② executor ✅ #4877"]
  end
  subgraph parallel["可并行 lane"]
    T01["T01 specta 接入<br/>与类型导出"]
    T02["T02 migration V2<br/>profiles revision 3"]
    T03["T03 ports +<br/>ProfileFileService"]
    T06["T06 RuntimeBuilder<br/>(add-only, 不切换)"]
  end
  subgraph actor["actor lane"]
    T04["T04 ProfilesActor<br/>+ ProfilesClient 核心事务"]
    T05["T05 RefreshRemote +<br/>Scheduler + watcher"]
  end
  subgraph flagday["原子切换组(同 PR, 串行)"]
    T07["T07 composition root<br/>+ facade 接线"]
    T08["T08 IPC BC 切换<br/>(13→16 条)"]
    T09["T09 前端适配"]
    T10["T10 legacy 清算"]
  end
  T11["T11 端到端验证<br/>+ 文档收尾"]
  P2 --> T06
  T01 --> T06
  T03 --> T04 --> T05
  T02 --> T07
  T04 --> T07
  T05 --> T07
  T06 --> T07
  T01 --> T08
  T07 --> T08 --> T09 --> T10 --> T11
```

**并行性**: T01 / T02 / T03 三者零文件重叠,可同时开工;T06 依赖 P2+T01;T04→T05 串行(同文件);切换组 T07–T10 严格串行。

---

## 2. 任务总表

| #   | 任务                                | scope(一句话)                                                   | 建议 commit                                                                | 依赖            | design 锚点      |
| --- | ----------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------------- | --------------- | ---------------- |
| T01 | specta 接入与类型导出               | tauri 依赖 nyanpasu-config,新类型逐 variant 导出 TS             | `feat(tauri): export nyanpasu-config profile types via specta`             | —               | §3 D11, §11      |
| T02 | migration profiles revision 3       | legacy profiles.yaml → clean schema(Value 层)                   | `feat(migration): add profiles revision 3 legacy-to-clean schema step`     | —               | §10, 图 13.4     |
| T03 | ports + ProfileFileService          | 三个窄 trait + fs/http 具体实现                                 | `feat(tauri): add profile fs/subscription ports and file service`          | —               | §7               |
| T04 | ProfilesActor + ProfilesClient      | 事务化状态归属 + 全部同步写消息                                 | `feat(tauri): add ProfilesActor with transactional profile state`          | T03             | §6, 图 13.1/13.5 |
| T05 | RefreshRemote + scheduler + watcher | 下载-提交分离、定时 reconcile、External 监听                    | `feat(tauri): add remote update scheduler and external watchers`           | T04             | §7, 图 13.3      |
| T06 | RuntimeBuilder(add-only)            | executor 输入组装 + golden 对照,不动调用点                      | `feat(tauri): add RuntimeBuilder over runtime pipeline executor`           | P2, T01         | §8, 图 13.2      |
| T07 | composition root + facade 接线      | spawn actor、facade 方法、rebuild 链路切换                      | `feat(tauri): wire ProfilesActor and RuntimeBuilder into composition root` | T02,T04,T05,T06 | §5, §6.4         |
| T08 | IPC BC 切换                         | 13 条旧命令 → 16 条 thin adapter                                | `feat(tauri)!: rewrite profile IPC commands against NyanpasuClient`        | T01, T07        | §9               |
| T09 | 前端适配                            | 新绑定 + current 单值化 + 最小 Composition 交互                 | `feat(frontend)!: adapt profiles UI to single current and new bindings`    | T08             | §11              |
| T10 | legacy 清算                         | 删 config/profile/\*\*、Config::profiles()、ProfilesJobGuard 等 | `refactor(tauri)!: remove legacy profiles types and accessors`             | T09             | §14 T3.8, §16    |
| T11 | 端到端验证 + 文档收尾               | e2e 冒烟、roadmap 状态行、台账 B8 登记                          | `docs: update actor migration roadmap for PR-3`                            | T10             | §15, §16         |

---

## 3. 任务卡

### T01 — specta 接入与类型导出

**目标**: tauri crate 依赖 `nyanpasu-config`,新 profile 域类型可生成 TS 绑定;旧代码零行为变化(add-only)。

**Files**:

- Modify: `backend/tauri/Cargo.toml`(加 `nyanpasu-config` workspace 依赖)
- Modify: `backend/tauri/src/lib.rs`(specta builder 注册新类型)
- Generated: `frontend/interface/src/ipc/bindings.ts`(仅新增类型,命令未变)

**Interfaces — Produces**(T08/T09 依赖):

- TS 侧实际导出的 PR-3 profile 域命名类型:`ProfileDocument`(`nyanpasu_config::profile::Profiles` 的 collision-safe 导出名;旧 `Profiles` 仍为 legacy profile DTO)、`ProfileItem`/`ProfileItem_Deserialize`/`ProfileItem_Serialize`、`ProfileMetadata`/`ProfileMetadata_Deserialize`/`ProfileMetadata_Serialize`、`ProfileDefinition`/`ProfileDefinition_Deserialize`/`ProfileDefinition_Serialize`、`ConfigDefinition`/`ConfigDefinition_Deserialize`/`ConfigDefinition_Serialize`、`FileConfig`/`FileConfig_Deserialize`/`FileConfig_Serialize`、`CompositionConfig`/`CompositionConfig_Deserialize`/`CompositionConfig_Serialize`、`TransformDefinition`/`TransformDefinition_Deserialize`/`TransformDefinition_Serialize`、`OverlayTransform`/`OverlayTransform_Deserialize`/`OverlayTransform_Serialize`、`ScriptTransform`/`ScriptTransform_Deserialize`/`ScriptTransform_Serialize`、`ScriptRuntime`、`ProfileSource`/`ProfileSource_Deserialize`/`ProfileSource_Serialize`、`LocalBinding`/`LocalBinding_Deserialize`/`LocalBinding_Serialize`、`ExternalMode`、`MaterializedFile`/`MaterializedFile_Deserialize`/`MaterializedFile_Serialize`、`ProfileRemoteOptions`(旧 `RemoteProfileOptions` 仍为 legacy DTO)、`ProfileSubscriptionInfo`(旧 `SubscriptionInfo` 仍为 legacy DTO)、`TransformOwner`、`CompositionMemberRole`。
- 透明 newtype 实际导出为命名别名:`ProfileId = string`、`ManagedProfilePath = string`、`ExternalProfilePath = string`。
- Patch / error 实际导出:`ProfileMetadataPatch`/`ProfileMetadataPatch_Deserialize`/`ProfileMetadataPatch_Serialize`、`RemoteProfileOptionsPatch`/`RemoteProfileOptionsPatch_Deserialize`/`RemoteProfileOptionsPatch_Serialize`、`ProfileValidationError`。`double_option` 三态字段通过 serialize/deserialize patch 形态保留。
- **维护注意(T01 审查发现)**: `ProfileDocument`/`ProfileRemoteOptions`/`ProfileSubscriptionInfo` 来自 `#[specta(remote = ...)]` mirror 结构(真实 struct 不再 derive `Type`)——域模型字段变更时必须**手动同步 mirror**,导出测试只断言类型名、不校验字段形状(漂移不会被编译或 CI 拦截);T08 plan 时评估补充字段形状断言。

**验证**:

- `cargo build -p clash-nyanpasu` 绿;TS 绑定生成命令成功且产物含全部命名类型
- CI TS diff 检查在位(绑定产物入库,diff 即 fail)
- 风险探针:specta 2.x 对嵌套 tagged enum 的推导问题**在本任务暴露**(design §17 风险 2)——若推导失败,方案调整只影响本卡

**单独 plan 时读**: design §11、D11;`backend/nyanpasu-config/src/profile/` 各类型的 serde/specta 属性。

---

### T02 — migration V2 `profiles` revision 3

**目标**: 注册 revision 3 step,把 legacy `profiles.yaml` 在 Value 层转换为 clean schema;`.bak` 备份;歧义显式失败;幂等重入。

**Files**:

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`(现 revision 1 `:47` / revision 2 `:114` 之后新增)
- Create: 迁移 fixtures(旧格式样本 YAML,建议随测试内联或 `tests/fixtures/`)

**规则清单**(全部来自 design §10,plan 时逐条转为测试):

- 类型映射四则;字段移动五组(`chain→global_transforms`、`local/remote.chain→File.transforms`、`script_type→runtime`、`file+updated→MaterializedFile`、`url/option/extra→ProfileSource::Remote`)
- `extra.expire:0→None`;`update_interval→update_interval_minutes`;URL-file→Remote(design §14.2)
- `local.symlinks: Some(target)` → `LocalBinding::External{target, mode: symlink}`(legacy field,guide §6 未列出;non-absolute target 显式失败)
- `remote.option` legacy defaults: absent key => `{with_proxy:false, self_proxy:true, 120}`;present missing fields => false/false;`update_interval == 0` 显式失败
- `option: null` 与 `URL 文件 + symlinks` 组合均显式失败(审查修复)
- multi-current:`[]→None`、`[a]→Some(a)`、`[a,b,c]→CompositionConfig{base:Some(a),extend:[b,c]}` + 碰撞安全 uid,顺序原样;成员无法映射 → **显式失败**(`MigrationError{uid, field_path}`)
- 收尾:反序列化为新 `Profiles` → `validate()` → 原子写回

**Interfaces — Produces**: revision 3 落账后,`profiles.yaml` 为新 schema(T07 spawn 前提);`MigrationStep` 实现遵循 `core/migration/mod.rs:83` trait(含 `rollback`)。

**验证**:

- fixtures 覆盖 design §10 全规则 + clean-design §18 第 24–27 条;幂等重入测试;`.bak` 生成断言
- 仿 `runner.rs:367` 的端到端样板(1.6.1 全量旧样本 → revision 3 → 新 schema 可 validate)
- `cargo test -p clash-nyanpasu migration` 绿

**单独 plan 时读**: design §10;guide §6;clean-design §14;`core/migration/{mod.rs, registry.rs, runner.rs:367, modules/profiles.rs}`。

---

### T03 — ports + ProfileFileService

**目标**: 定义消费方拥有的三个窄 trait 并提供具体实现;纯增量,无调用方。

**Files**:

- Create: `backend/tauri/src/state/profiles/ports.rs`(或 `state/profiles.rs` 内 mod;plan 时定,保持 Tauri-free)
- Create: `backend/tauri/src/service/profile_file.rs`(`ProfileFileService`)
- Modify: `backend/tauri/src/service/mod.rs` / `lib.rs`(模块声明)

**Interfaces — Produces**(T04/T05/T07 依赖,签名以此为准):

```rust
#[cfg_attr(test, mockall::automock)]
pub trait ProfileFsPort: Send + Sync + 'static {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String>;
    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()>;
    /// Idempotent: removing a missing file succeeds.
    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    /// Read an External binding target for Mirror synchronization.
    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String>;
    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    fn ensure_symlink(&self, path: &ManagedProfilePath, target: &ExternalProfilePath) -> anyhow::Result<()>;
}
#[cfg_attr(test, mockall::automock)]
pub trait SubscriptionFetcher: Send + Sync + 'static {
    async fn fetch(&self, url: &Url, options: &RemoteProfileOptions) -> anyhow::Result<FetchedSubscription>;
}
#[cfg_attr(test, mockall::automock)]
pub trait RebuildNotifier: Send + Sync + 'static {
    fn request_rebuild(&self);
}
pub struct FetchedSubscription {
    pub content: String,
    pub filename: Option<String>,
    pub subscription: SubscriptionInfo,
}
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}
// ProfileFileService::new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>)
// — 同时 impl ProfileFsPort + SubscriptionFetcher;T07 composition root 提供 SelfProxyPortSource。
pub fn normalize_yaml_document(content: &str) -> anyhow::Result<String>;
```

**验证**:

- 单测(tempdir):原子写、`ensure_not_symlink` 对符号链接拒绝、YAML 规范化读、`fetch` 网络超时自管(mock http 或 feature-gate)
- `state/profiles/` 无 `tauri::*` / `crate::config` import(grep 断言)

**单独 plan 时读**: design §7、D9、D10;`utils/path.rs:40,94,99`(PathResolver);clean-design §9 末段(符号链接防御)。

---

### T04 — ProfilesActor + ProfilesClient(核心事务)

**目标**: profiles 状态归属 actor;全部**同步写消息**落地(Add/Delete/Reorder/PatchMetadata/PatchRemoteOptions/ReplaceDefinition/SetCurrent/SetGlobalTransforms/Replace)+ Get 读;七步事务 + 依赖索引 + `CommitReport`。**不含** RefreshRemote/scheduler/watcher(→T05,缩小审查半径)。

**Files**:

- Create: `backend/tauri/src/state/profiles.rs`(actor;若 T03 用了子目录则为 `state/profiles/mod.rs` + `actor.rs`)
- Create: `backend/tauri/src/client/profiles.rs`(`ProfilesClient`)
- Modify: `backend/tauri/src/state/mod.rs`、`client/mod.rs`(仅模块声明,facade 方法留给 T07)

**Interfaces — Consumes**: T03 三 trait + `PersistentStateManager<Profiles>`(`nyanpasu-core/src/state/manager/persistent_state.rs:128`)+ `nyanpasu-config` patch.rs 分层 mutator + `ProfileDependencyIndex`(`dependency.rs:10`)。

**Interfaces — Produces**(T05/T07/T08 依赖):

```rust
pub struct ProfilesClient { /* ActorRef 私有 */ }
impl ProfilesClient {
    pub async fn get(&self) -> Result<Arc<Profiles>, ProfilesError>;                    // call(_, Some(PROFILES_READ_TIMEOUT))
    pub async fn add(&self, req: NewProfileRequest, initial_file: Option<String>) -> Result<CommitReport, ProfilesError>;
    pub async fn delete(&self, uid: ProfileId) -> Result<CommitReport, ProfilesError>;  // 引用保护
    pub async fn reorder(&self, op: ReorderOp) -> Result<CommitReport, ProfilesError>;
    pub async fn patch_metadata(&self, uid: ProfileId, patch: ProfileMetadataPatch) -> Result<CommitReport, ProfilesError>;
    pub async fn patch_remote_options(&self, uid: ProfileId, patch: RemoteProfileOptionsPatch) -> Result<CommitReport, ProfilesError>;
    pub async fn replace_definition(&self, uid: ProfileId, definition: ProfileDefinition) -> Result<CommitReport, ProfilesError>;
    pub async fn set_current(&self, current: Option<ProfileId>) -> Result<CommitReport, ProfilesError>;
    pub async fn set_global_transforms(&self, ids: Vec<ProfileId>) -> Result<CommitReport, ProfilesError>;
    pub async fn replace(&self, profiles: Profiles) -> Result<CommitReport, ProfilesError>;
}
pub struct CommitReport { pub snapshot: Arc<Profiles>, pub affects_current: bool, pub warnings: Vec<String> }
pub struct NewProfileRequest { pub metadata: ProfileMetadata, pub definition: ProfileDefinition }  // uid 服务端生成(D13)
pub enum ReorderOp { Move { active: ProfileId, over: ProfileId }, ByList(Vec<ProfileId>) }
pub enum ProfilesError { ProfileNotFound, ProfileInUse { referrers: Vec<ProfileId> }, ProfileHasNoFile,
                         ValidationFailed(Vec<ProfileValidationError>), NotARemoteProfile,
                         FileNotWritable { reason: String }, RefreshFailed { message: String },
                         Persist(String), Rpc(String) }
pub const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);
pub struct ProfilesActorArgs { pub manager: PersistentStateManager<Profiles>,
                               pub fs: Arc<dyn ProfileFsPort>, pub fetcher: Arc<dyn SubscriptionFetcher>,
                               pub notifier: Arc<dyn RebuildNotifier> }
```

**2026-07-06 契约修正(T04 实物,下游以此为准)**:

- `ProfilesClient::new(profiles_path, fs, fetcher, notifier)` 是构造边界:若 `profiles_path` 存在则 `load()`;否则 `from_state(Profiles::default())`;随后立即 `validate()` fail-fast 再 spawn actor。`ProfilesActorArgs` 不含 `PathResolver` 或 `initial`。
- 所有写 handler 仍走 clone→mutate→`validate()`→持久化→commit+重建索引→post-commit 副作用→`CommitReport`;post-commit 文件副作用失败写入 `CommitReport.warnings`,不回滚已持久化状态。
- `ProfilesError::Persist(String)` 专指持久化/提交失败;`Rpc(String)` 仅表示 actor call/reply/timeout 层失败。
- Add 服务端生成 uid:`Config` 用 `c` 前缀、`Transform` 用 `t` 前缀,后接 `nanoid(11)`;忽略请求里的 materialized 路径并改写为规范 `{uid}.{ext}`。`FileConfig`/`OverlayTransform` 用 `.yaml`;`ScriptRuntime::JavaScript` 用 `.js`;`ScriptRuntime::Lua` 用 `.lua`。`ExternalMode::Symlink` 只做 post-commit `ensure_symlink`;Remote 与 Mirror 的初始内容同步由 T05 的 RefreshRemote/watcher reconcile 承接。
- **(2026-07-06 审查修复)materialization 元数据由服务端所有**:Add 一律重置 `updated_at = None`(Remote 另重置 `subscription = default`);`ReplaceDefinition` 同样把传入定义的路径改写为规范 `{uid}.{新类型 ext}`——source 槽位(Managed/External/Remote 判别 + 规范路径)不变时沿用旧存储的 materialized 元数据、忽略客户端传值;槽位变化时重置元数据,旧路径与新规范路径不同(或新定义无 source,如换成 Composition)时 post-commit `Remove` 清理孤儿文件,新引入 External Symlink 绑定时 post-commit `ensure_symlink`(与 Add 对齐)。
- **(2026-07-06 审查修复)错误形态**:`ProfileInUse { referrers, current: bool, global_transforms: bool }`(document 级引用不再以空列表歧义表达);新增 `InvalidReorderList { reason }`(ByList 长度不符/重复 uid),未知 uid 仍报 `ProfileNotFound`。
- **(2026-07-06 审查记录)** 真实文件事件冒烟测试 `external_watcher_smoke_mirror_real_file_event` 标 `#[ignore]`(CI 抖动;注入式测试保有覆盖);Mirror 同步在 actor handler 内做同步 fs I/O,target 位于网络盘时有阻塞风险——候选硬化项(spawn_blocking 化),T07 阶段评估。
- `affects_current` 判定表:

| 消息                                                                  | 规则                                                              |
| --------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `Get`                                                                 | 不适用                                                            |
| `Add` / `Delete` / `Reorder` / `PatchMetadata` / `PatchRemoteOptions` | `false`                                                           |
| `SetCurrent`                                                          | before/after `current` 不同                                       |
| `SetGlobalTransforms`                                                 | before/after `global_transforms` 不同                             |
| `ReplaceDefinition`                                                   | before/after 当前闭包不同,或被替换 uid 在 before/after 当前闭包内 |
| `Replace`                                                             | `true`                                                            |

当前闭包 = `current` + 若 current 为 Composition 则含 base/extend 成员 + 这些 config 的 scoped transforms + `global_transforms`;`current == None` 时闭包仅为 `global_transforms`。

**验证**(mock ports + tempdir manager spawn,不 sleep):

- 每条消息 happy path;`ValidationFailed` 不落盘不改内存;`Delete` 五类引用全部拒绝(`ProfileInUse.referrers` 正确);`affects_current` 判定(改 current 传递闭包内/外各一例)
- 写路径 `call(_, None)`、读路径 `call(_, Some)` 有断言;Add 写初始文件、Delete 按 binding 清理(Managed 删 / Symlink 只删链接 / Mirror 删副本 / Composition 无操作)
- `cargo test -p clash-nyanpasu profiles` 绿

**单独 plan 时读**: design §6 全部、图 13.1/13.5;patch-interface §4–6;clean-design §13/§16/§17;PR-2b spec §6–7(actor 同构样板);现 `state/verge.rs`(消息/manager 用法参考)。

---

### T05 — RefreshRemote + RemoteUpdateScheduler + External watcher

**目标**: 订阅刷新与外部文件同步纳入 actor:下载-提交分离(`RefreshRemote` 挂起 reply → 子任务下载 → `CommitRefreshed` 串行提交)、定时表 reconcile、Symlink/Mirror watcher、后台提交经 `RebuildNotifier` 通知。

**Files**:

- Modify: `backend/tauri/src/state/profiles.rs`(新增消息 `RefreshRemote`/`CommitRefreshed`/`ExternalFileChanged`、`pending_refresh` 表、`post_start` 首次 reconcile、scheduler 子任务)
- Create(如拆文件): `backend/tauri/src/state/profiles/scheduler.rs`
- Modify: `backend/tauri/src/client/profiles.rs`(+`refresh(uid, Option<RemoteProfileOptionsPatch>) -> Result<CommitReport, ProfilesError>`)

**Interfaces — Consumes**: T04 全部(含 2026-07-06 契约修正:`CommitReport.warnings` 降级通道、`Persist` 错误、Add 规范路径与 Remote/Mirror 初始内容延后规则) + T03 `SubscriptionFetcher`/`ProfileFsPort`/`RebuildNotifier`。
**Interfaces — Produces**: `ProfilesClient::refresh(...)`(T07/T08 的 `update_profile` 链路);scheduler/watcher 对外不可见(actor 内部)。

**行为要点**(plan 时逐条转测试):

- handler 内零网络 I/O;下载任务超时自管(options 派生);失败路径必结清挂起 reply
- reconcile 幂等:新增/修改/Local↔Remote 切换/删除四类 diff(clean-design §18 第 22 条)
- watcher:Symlink 监听 target 真实路径、Mirror 变化→临时文件→校验→原子替换(design §7;clean-design §10)
- `CommitRefreshed` 时 uid 已被删除 → 丢弃结果、结清 reply(design §17 竞态行)
- 后台提交且 `affects_current` → `notifier.request_rebuild()` 恰好一次

**2026-07-06 契约补遗(T05 实物,下游以此为准)**:

- `ProfilesActorMessage::RefreshRemote { uid, patch, reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>> }`:手动 `ProfilesClient::refresh(...)` 使用 `Some(reply)`;scheduler 后台刷新使用 `None`。`patch` 先以普通写事务更新 remote options,失败则立即结清 `Some(reply)`。
- 同一 uid 已有 `pending_refresh` 时,`Some(reply)` 返回 `ProfilesError::RefreshFailed { message: "refresh already in progress" }`;`None` 后台触发静默丢弃。所有已登记 pending 的路径必须在 success/failure/deleted race 中结清。
- `RefreshRemote` handler 不做网络 I/O:它快照 remote 参数、登记 pending reply,然后 `tokio::spawn` 执行 fetch/校验/`ensure_not_symlink`/`write_atomic`;子任务只回 cast `CommitRefreshed { uid, outcome }`,由 actor 串行提交 `updated_at` 与 `subscription`。
- `CommitRefreshed` 发现 uid 已删除时,若 outcome 为 success, best-effort 清理 `{uid}.yaml` / `{uid}.js` / `{uid}.lua` 三个可能 orphan;`Some(reply)` 以 `RefreshFailed { message: "profile deleted during refresh" }` 结清。
- 后台刷新(`reply == None`)只有在成功提交且 `CommitReport.affects_current` 时调用 `RebuildNotifier::request_rebuild()`;每次 background-driven commit 恰好一次。手动 refresh 的 reply 路径不在 actor 内直接 notify,由 T07 facade 按 `CommitReport` 统一编排。
- `RemoteUpdateScheduler` 为每个 remote uid 持有独立 tokio timer task;每次 committed write 后按 uid+interval diff add/remove/update。`post_start` 会 catch-up overdue remote: `updated_at == None` 视为 overdue,否则 `now - updated_at >= update_interval_minutes` 触发一次后台 refresh。
- `ExternalWatchers` 使用 `notify-debouncer-full` 按 External binding 建 watcher。Symlink 监听 canonicalized target(失败回退 raw target)且事件只 bump `updated_at`;Mirror 事件先 `ProfileFsPort::read_external`、按 profile definition 校验、`write_atomic` 到 managed materialization,再 bump `updated_at`。成功提交且影响 current 时 notify 恰好一次;读取/校验/写入失败只记录 warn,不改状态、不 notify。
- 测试入口保留 `#[cfg(test)]` client helper 直接 cast `ExternalFileChanged`,用于覆盖 handler 语义;真实 watcher smoke 使用 bounded `<=5s` 文件事件测试,当前未标 `#[ignore]`。

**验证**: mock fetcher 注入可控延迟/失败;watcher 用 tempdir 真实文件事件或注入触发;全程无 sleep(用消息 ack/通道)。

**单独 plan 时读**: design §7、图 13.3、D8/D9、§18 O1/O3;clean-design §9/§10;现 `core/tasks/jobs/profiles.rs:49-139`(被取代者的 cron diff 语义参考)。

---

### T06 — RuntimeBuilder(add-only,不切换调用点)

**目标**: 新建 `RuntimeBuilder` 纯 service + 两个 executor 适配器;golden 对照测试证明与旧 `enhance()` 产物等价。**本卡不改 `Config::generate()`、不删旧 enhance 逻辑**——行为切换在 T07,保证本卡纯增量、可独立回滚。

**Files**:

- Create: `backend/tauri/src/enhance/runtime_builder.rs`(`RuntimeBuilder` + `RuntimeBuildInput`)
- Create: `backend/tauri/src/enhance/content_source.rs`(`FsProfileContentSource: ProfileContentSource`,按 `ManagedProfilePath` 读物化文件)
- Modify: `backend/tauri/src/enhance/script/`(为现 boa/lua runner 加 `ScriptRunner` trait impl 包装,不动原逻辑)
- Create: golden fixtures(旧行为样本:单 current、multi-current→Composition、scoped chain、global chain、builtin 门控、HANDLE_FIELDS overlay、whitelist 过滤)

**Interfaces — Consumes**(2026-07-06 勘误,#4877 实物:`backend/nyanpasu-config/src/runtime/executor/{mod,ports,artifact}.rs`):

```rust
// executor 入口(mod.rs:196)
pub fn execute(inputs: &RuntimePipelineInputs<'_>, content: &dyn ProfileContentSource, runner: &dyn ScriptRunner)
    -> Result<RuntimeArtifact, RuntimePipelineError>;
pub struct RuntimePipelineInputs<'a> { pub profiles: &'a Profiles, pub target: ExecutionTarget,
    pub guard: GuardInputs<'a>, pub whitelist_enabled: bool, pub tun: TunParams,
    pub builtin_transforms: &'a [BuiltinTransform] }
pub enum ExecutionTarget { Selected(ProfileId), Bare }                     // Bare = current 为 None 的裸配置路径
pub struct GuardInputs<'a> { pub overrides: &'a ClashGuardOverrides, pub ports: ResolvedPortBindings }
pub struct ResolvedPortBindings { pub mixed_port: u16, pub port: Option<u16>,
    pub socks_port: Option<u16>, pub external_controller: Option<String> } // 端口探测 IO 不进 executor
pub struct TunParams { pub enable: bool, pub flavor: TunFlavor, pub windows_fake_ip_filter: bool }
pub enum TunFlavor { ClashRs, Standard { stack: TunStack } }               // 含 Premium+Mixed→Gvisor 降级,由调用方推导
pub struct BuiltinTransform { pub name: String, pub runtime: ScriptRuntime, pub source: String }
// ports.rs:ScriptRunner 三方法 run / eval_item_predicate / eval_item_expr;run 返回 ScriptRunOutcome{result, logs}
// artifact.rs:RuntimeArtifact { final_config: Arc<ConfigValue>, graph, step_logs: Vec<StepLog>, applied_fields }
```

另消费:`enhance/chain.rs:145` builtin 门控表(按 `ClashCore` bitflags 组装 `Vec<BuiltinTransform>`)。

**Interfaces — Produces**(T07 依赖;字段实名 plan 时锚定,变更回写本卡):

```rust
// RuntimeBuilder 职责 = 把域快照确定性地降解为 RuntimePipelineInputs:
// TunFlavor 推导(含 Premium+Mixed→Gvisor)、ClashCore 门控 builtins、cfg!(windows) 传参、
// current=None → ExecutionTarget::Bare。端口解析(IO)不在 builder 内,作为 ResolvedPortBindings 传入。
pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,               // ProfilesClient 快照
    pub clash: ClashConfig,                    // ClashConfigClient 快照(guard overrides/tun/enable_clash_fields)
    pub app: NyanpasuAppConfig,                // ApplicationClient 快照(core 选择 + builtin 开关)
    pub resolved_ports: ResolvedPortBindings,  // 调用方预解析(T07:composition/facade 侧)
}
impl RuntimeBuilder {
    pub fn build(input: &RuntimeBuildInput, content: &dyn ProfileContentSource, scripts: &dyn ScriptRunner)
        -> Result<RuntimeArtifact, RuntimeBuildError>;   // Validation(Vec<ProfileValidationError>) | Pipeline(RuntimePipelineError)
}
```

**2026-07-06 契约修正(T06 实物,下游以此为准)**:

- 落点实名:`enhance/runtime_builder.rs`(`RuntimeBuilder`/`RuntimeBuildInput`/`RuntimeBuildError` + `builtin_transforms_for(core)`/`derive_tun_flavor(core, stack)` 公开可测)、`enhance/content_source.rs`(`FsProfileContentSource::new(profiles_dir: PathBuf)`)、`enhance/script/adapter.rs`(`EnhanceScriptRunner::new()`,三方法 `ScriptRunner`,复用 `RunnerManager` + `create_lua_context`)。均从 `crate::enhance` 顶层 `pub use`。
- `build()` 前置 `profiles.validate()` 防御(executor 契约要求已验证输入)→ `RuntimeBuildError::Validation`。
- builtin 门控实现为切片匹配表(未给 tauri 引入 enumflags2);顺序与 gating 与 `chain.rs:170-175` 逐位一致,含 `clash_rs_comp` 不含 `ClashRsAlpha`、tun `ClashRs` 分支不含 Alpha 两处 legacy 怪癖(修复属行为变更,需另走勘误)。
- `TunStack` 实际路径 = `nyanpasu_config::clash::config::tun_stack::TunStack`。
- `EnhanceScriptRunner` 自持 current-thread runtime(`run` 同步阻塞)——**`RuntimeBuilder::build` 必须在阻塞上下文调用**(T07 统一 `spawn_blocking`);禁止在 async worker 线程直接调用。
- golden 现状:端到端不变量测试(真实 boa + 真实文件源:脚本生效/guard 端口注入/whitelist-off 保键/step_logs 锚定)+ executor 侧 PR-3-pre② parity 套件背书;**snapshot 期望文件套件列为 T07 pre-flight 跟进项**(多场景:Composition、global chain、builtin 门控、whitelist-on)。
- 评审处置(antigravity 2026-07-06;codex 后端故障,其评审延期):①"`block_on` 在 `spawn_blocking` 内 panic"判**误报**——tokio 仅在 async worker 线程禁止 `block_on`,blocking 池线程允许(`Handle::block_on` 官方文档模式、reqwest::blocking 同构先例),阻塞上下文契约见上条;②"每次 `run` 重建 `RunnerManager` 是性能回退"**不成立**——`JSRunner` 为 unit struct,boa `Context`(`Rc<RefCell<_>>`,!Send)legacy 同样每脚本新建,manager 仅空 HashMap;③BitFlags 门控为有意偏离(新域 `ClashCore` 无 `#[bitflags]` derive,跨 crate 改动超卡范围)——新增 core 变体时切片表需手工同步,由门控测试兜底;④**T07 加固项**:`ManagedProfilePath` 反序列化侧缺 `..` 组件拒绝(防御纵深),须用 `Component::ParentDir` 组件检查而非 `starts_with`(lexical 比较对未归一化路径无效)。

**验证**:

- golden 对照:同输入下 `RuntimeBuilder::build` 产物与旧 `enhance()` 等价(design §15「最高风险项」;PR-3-pre② T3p.6 fixtures 复用为回归)
- `RuntimeArtifact.step_logs` 能还原旧 `postprocessing_output` 消费需求
- 纯度断言:`runtime_builder.rs` 无 `Config::` / `tauri::` import(D12 取数不在本卡——输入全部显式传参)

**单独 plan 时读**: design §8/§19、图 13.2、D7/D12;`backend/nyanpasu-config/src/runtime/executor/`(实物:`mod.rs`/`ports.rs`/`artifact.rs`;`tests/{golden,parity}.rs` fixtures 可复用);`docs/superpowers/specs/2026-07-04-runtime-pipeline-executor-design.md` §19 勘误;`enhance/mod.rs:22-104`、`enhance/chain.rs:59-160`(旧语义源)。

---

### T06A — 评审加固 + golden 基线(add-only,排 T07 前;2026-07-06 增补)

**目标**: 落地 T06 评审处置遗留的三项加固,并在 `Config::generate()` 切换前锁定 RuntimeBuilder 行为基线。全部 add-only,不入 §4 原子切换组,可独立提交。

**内容**:

1. `ManagedProfilePath` 构造/反序列化拒绝 `..`:用 `Component::ParentDir` 组件检查(**不得**用 `Path::starts_with`——lexical 比较对未归一化路径无效,T06 评审处置④)。落点 `backend/nyanpasu-config`(路径类型),含拒绝用例单测。
2. Mirror 同步阻塞 I/O 加固:核实 T05 watcher Mirror 分支(`state/profiles/scheduler.rs`)的复制/校验是否已在阻塞线程执行;否则移入 `spawn_blocking`。plan 时以实测定改动量(可能为零改动 + 测试确认)。
3. RuntimeBuilder golden snapshot 文件套件(T06 附录跟进项):固定输入(样本 profiles + clash/app 快照 + 固定 `ResolvedPortBindings`)驱动 `RuntimeBuilder::build`,`final_config` 序列化 YAML 作为期望文件入库;场景至少覆盖 Composition、global chain、builtin 门控、whitelist-on。T07 切换后同套件必须仍绿。

**Interfaces — Consumes**: T06 `RuntimeBuilder`/`FsProfileContentSource`/`EnhanceScriptRunner` 实物。
**Interfaces — Produces**: golden 套件(T07 回归安全网);`ManagedProfilePath` 构造语义收紧(拒绝 `..`;T02 迁移生成的 `{uid}.{ext}` 形态路径不受影响)。

**验证**:

- `ManagedProfilePath::new("../evil")` 等含 `..` 组件的构造被拒(含反序列化路径)
- golden 套件可重复运行、期望文件稳定;`cargo test` 全绿

**单独 plan 时读**: T06 卡「2026-07-06 契约修正」评审处置条;`state/profiles/scheduler.rs`(Mirror 分支现状);`enhance/runtime_builder.rs` 现有测试(`base_input()` 可复用)。

**2026-07-06 执行修正(T06A 实物)**:

- 第 1 项按实物收缩:`..`/穿越拒绝防御与构造侧测试**原已在位**(nyanpasu-config path.rs:30-40/60-67 + validation.rs:166-171;T06 评审处置④的「反序列化侧缺失」表述与实物不符)——本卡实际交付 = profiles 文档反序列化面的回归钉测试(validation.rs 尾部)。
- golden 套件实名:`enhance/golden.rs` + `enhance/fixtures/golden/{composition_global_chain,builtin_mihomo,builtin_clash_rs,whitelist_on}.yaml`;重铸 `GOLDEN_BLESS=1 cargo test -p clash-nyanpasu golden_`;确定性 = 固定 `secret: golden-secret` + tun off。**T07 切换后本套件须原样全绿,改 fixtures = 行为回归须勘误。**
- Mirror 同步已移入 `spawn_blocking`(actor.rs `ExternalFileChanged` Mirror 分支;消息顺序不变,warn 日志逐字保留)。

**2026-07-07 评审处置(codex 79/100 + antigravity 98/100 APPROVE,无 Critical)**:

- Major(已修):golden builtin 两例经真 boa 执行,而 `boa_utils::set_logger` 为进程级全局(js.rs 注释自证非并发安全),与 `runtime_builder.rs` 日志断言 e2e 测试并行有窃取/漏失竞态 → js.rs 增 `BOA_LOGGER_LOCK` 在 `spawn_blocking` 闭包内串行整段 boa 执行(生产侧脚本本就经 actor 串行,无性能回退)。
- Suggestion(已采纳):`golden_input` 显式 `enable_clash_fields = false`,基线自文档化。
- Minor(遗留,记 T07 预备项):Mirror 失败三分支(读失败/校验失败/镜像写失败)缺「不 commit updated_at、不触发 rebuild」的断言钉。
- antigravity 2 条 Info(run_write 后置文件操作仍同步、golden 测试助手与 runtime_builder 测试重复)均记为未来事项,不入本卡。

---

### T07 — composition root + facade 接线(⚠️ 切换组起点)

**目标**: spawn `ProfilesActor` 进 composition root;`NyanpasuClient` 暴露全部 profiles 域方法 + `rebuild_running_config()`;`Config::generate()` 改调 RuntimeBuilder;`RebuildNotifier` 接线。**自本卡起应用进入 BC 中间态**(旧 IPC 仍在但底层已切换,见 §4)。

**Files**:

- Modify: `backend/tauri/src/setup.rs` / `client/mod.rs`(spawn 顺序:migration 子进程已完成 → 构造 ProfileFileService → spawn ProfilesActor → 构造 ProfilesClient → facade;`RebuildNotifier` 具体实现接到 facade 重建入口,注意用 `Weak`/channel 避免循环持有)
- Modify: `backend/tauri/src/config/core.rs:88`(`generate()` 改调 RuntimeBuilder;产物仍写 runtime draft + `generate_file`,两处标 `TODO(actor-migration)` B8)
- Modify: `backend/tauri/src/client/mod.rs`(新 facade 方法;旧 `patch_profiles_config:80` 本卡保留——删除在 T10)

**Interfaces — Consumes**: T02 revision 3 后的新 schema;T04 ProfilesClient 全部方法与 2026-07-06 契约修正(`CommitReport.warnings` 不代表事务失败,`affects_current` 按闭包规则触发 rebuild);T05 final refresh signature `ProfilesClient::refresh(uid: ProfileId, patch: Option<RemoteProfileOptionsPatch>) -> Result<CommitReport, ProfilesError>`;T06 `RuntimeBuilder`。

**Interfaces — Produces**(T08 依赖,方法名以此为准):

```rust
impl NyanpasuClient {
    pub async fn get_profiles(&self) -> Result<Arc<Profiles>>;
    pub async fn add_profile(&self, req: NewProfileRequest, initial_file: Option<String>) -> Result<ProfileId>; // 2026-07-06:返回服务端生成 uid(import 条件激活用)
    pub async fn delete_profile(&self, uid: ProfileId) -> Result<()>;
    pub async fn reorder_profile(&self, active: ProfileId, over: ProfileId) -> Result<()>;
    pub async fn reorder_profiles_by_list(&self, list: Vec<ProfileId>) -> Result<()>;
    pub async fn refresh_profile(&self, uid: ProfileId, patch: Option<RemoteProfileOptionsPatch>) -> Result<()>;
    pub async fn patch_profile_metadata(&self, uid: ProfileId, patch: ProfileMetadataPatch) -> Result<()>;
    pub async fn patch_remote_profile_options(&self, uid: ProfileId, patch: RemoteProfileOptionsPatch) -> Result<()>;
    pub async fn replace_profile_definition(&self, uid: ProfileId, definition: ProfileDefinition) -> Result<()>;
    pub async fn activate_profile(&self, uid: Option<ProfileId>) -> Result<()>;
    pub async fn set_global_transforms(&self, ids: Vec<ProfileId>) -> Result<()>;
    pub async fn get_profile_materialized_path(&self, uid: ProfileId) -> Result<PathBuf>;   // Composition → ProfileHasNoFile
    pub async fn read_profile_file(&self, uid: ProfileId) -> Result<String>;
    pub async fn save_profile_file(&self, uid: ProfileId, data: String) -> Result<()>;      // 仅 Local/Managed
    pub async fn rebuild_running_config(&self) -> Result<()>;   // 快照→RuntimeBuilder→runtime draft→CoreManager(TODO B8)
}
```

写方法内部统一模式:`CommitReport.affects_current == true` → 顺序调用 `rebuild_running_config()`(facade 编排,design §6.4)。

**验证**:

- 启动冒烟(顺序断言:migration 先于 spawn);`rebuild_running_config` 集成测试(mock 或真实 executor)
- 台账检查:本卡新增 TODO 注释恰好覆盖 design §8 所列两处(D12 已走 typed client 取数,无第三处;design §19)

**单独 plan 时读**: design §5/§6.4/§8、图 13.2;PR-2b spec §10.2(composition root 顺序样板);`setup.rs`/`lib.rs:120,362-398` 现状。

**2026-07-06 契约修正(现场盘点,下游以此为准)**:

- 接线简化(实物):`ProfileFileService::new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>)` 一个实例同时实现 `ProfileFsPort` + `SubscriptionFetcher`(`profile_file.rs:126/202`);`ProfilesClient::new(profiles_path: Utf8PathBuf, fs, fetcher, notifier)`(`client/profiles.rs:31`,`pub(crate)`)内部自行 spawn actor——无独立 spawn 步骤;T05 调度器/watcher 为 actor 生命周期内部细节,composition root 零额外接线。
- 本卡新增三个生产实现(盘点:全仓均无非测试实现):① `RebuildNotifier`(channel 接 facade 重建入口,接收侧去抖);② `SelfProxyPortSource`(D12:取 ClashConfig 快照 mixed port);③ 端口解析纯函数 `resolve_port_bindings(&ClashConfig, &NyanpasuAppConfig) -> ResolvedPortBindings`(random-port 等 legacy 语义 plan 时实测对齐)。
- 契约修正(§5.3,波及 T04 实物):`CommitReport` 增补 `created: Option<ProfileId>`(仅 Add 置值)——`import_profile` 条件自动激活需要服务端生成的 uid(`actor.rs:476` `generate_uid`);facade `add_profile` 改返 `Result<ProfileId>`(上方方法表 `Result<()>` 作废)。
- 文件三方法(`get_profile_materialized_path`/`read_profile_file`/`save_profile_file`)在 facade 层实现,不经 actor 消息:快照定位 + `ProfileFileService` 直读写(§9 BC 要点照旧:仅 Local/Managed 可写、Composition→`ProfileHasNoFile`);save 不自动 rebuild(维持 legacy:前端显式 `enhance_profiles`)。
- `generate()` 切换映射:`RuntimeArtifact→IRuntime` = `config←final_config`(ConfigValue→Mapping 投影)、`exists_keys←applied_fields`、`postprocessing_output←step_logs`(shape 转换 plan 时定,保持 `IRuntime` 对外 shape 与 `get_postprocessing_output` 返回类型不变);`rebuild_running_config()` 读完快照后全程 `spawn_blocking`(T06 附录阻塞契约),`FsProfileContentSource`/`EnhanceScriptRunner` 在闭包内每次构造。
- 连接中断(**用户决策 2026-07-06,有意偏离 legacy**):`rebuild_running_config()` 成功后统一调用 `ConnectionInterruptionService::on_profile_change()`(加 `TODO(actor-migration)` bridge 注释,其内读 `Config::verge()`)。legacy 仅 `patch_profiles_config_inner` 路径触发;新行为下删除当前 profile、订阅后台刷新、External 失效等 affects_current 提交同样触发。影响评估:`break_when_profile_change` 默认 false,仅影响显式开启用户,语义更贴近选项本意。台账判据由「恰好两处 TODO」改为「**恰好三处**」。
- 锚点修正:删「旧 `patch_profiles_config:80` 本卡保留」条——`NyanpasuClient` 无此方法(唯一同名物 `ipc.rs:284` 命令,归 T08);profiles 域作为 `NyanpasuClientInner` 第四成员(现 application/session_state/clash_config 三成员)。
- 迁移顺序现状已满足:`setup.rs:22-26` `run_pending()` 先于 client 构造,ProfilesClient 插入同一序列;存在双迁移机制(`lib.rs:120` 子进程 + `setup.rs` in-process),plan 时核实 rev3 实际生效路径。

---

### T08 — IPC BC 切换(13 → 16 条)

**目标**: `ipc.rs` 全部 profile 命令重写为 thin adapter;新增/拆分命令注册进 specta builder 与 handler 列表;域错误 → 命令错误映射。

**Files**:

- Modify: `backend/tauri/src/ipc.rs:102-382`(13 条命令按 design §9 表逐条替换;`patch_profiles_config`/`patch_profile` 删除,5 条新命令加入)
- Modify: `backend/tauri/src/lib.rs`(command 注册表 + specta 导出)
- Modify: `backend/tauri/src/feat.rs:441` 附近(`feat::update_profile` 调用点改走 facade;函数本体删除在 T10)
- Generated: `frontend/interface/src/ipc/bindings.ts`(命令面变化——**自本 commit 起前端类型检查红,直至 T09**,见 §4)

**Interfaces — Consumes**: T07 全部 facade 方法。
**Interfaces — Produces**: 16 条命令名(前端 T09 依赖):`get_profiles / enhance_profiles / import_profile / create_profile / reorder_profile / reorder_profiles_by_list / update_profile / delete_profile / activate_profile / set_global_transforms / patch_profile_metadata / patch_remote_profile_options / replace_profile_definition / view_profile / read_profile_file / save_profile_file`。

**验证**:

- 每条命令 = 解析 DTO → facade → 错误映射,**零业务编排**(CLAUDE.md §12 形状检查)
- IPC 集成测试:happy path + `ProfileInUse`/`ProfileHasNoFile`/`ValidationFailed` 映射
- `grep -n "Config::profiles()" backend/tauri/src/ipc.rs` 零命中

**单独 plan 时读**: design §9 全表(每行含 BC 要点);guide §5(逐命令现状签名)。

**2026-07-06 契约修正(现场盘点)**:

- 注册表锚点:命令注册 + specta 导出在 `specta_export.rs`(`build_specta_builder()`,profiles 条目 53–66 行),非 lib.rs(lib.rs 仅 199/238/249 三处消费 builder)。
- `feat::update_profile` 实际在 `feat.rs:444-447`;调用者两处:`ipc.rs:255`(本卡改走 facade)+ `core/tasks/jobs/profiles.rs:33`(T10 随整文件删除,本卡不动)。
- `import_profile` 条件自动激活:用 facade `add_profile` 返回的 `ProfileId`(T07 契约修正),激活调 facade `activate_profile`;连接中断由 `rebuild_running_config()` 统一触发(T07 用户决策),命令层零编排。
- `patch_profiles_config_inner`(`ipc.rs:288-310`)随两条旧命令在本卡删除,不留 T10。
- 现状 13 条命令清单实测与本卡一致;16 条目标名单不变。

---

### T09 — 前端适配

**目标**: 前端在新绑定下恢复类型检查绿 + profiles 页全功能;`current` 单值化;多选激活改为最小 Composition 创建交互。

**Files**(代表锚点,plan 时全量盘点):

- Regenerate: `frontend/interface/src/ipc/bindings.ts`
- Modify: `frontend/interface/src/ipc/use-profile.ts:167` 及同文件相关 hook(命令改名/拆分)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/profiles/` 下 `current` 消费点(如 `active-button.tsx:21` 的 `current?.find(...)` → `current === uid`)
- Modify: profile 编辑对话框(metadata / remote options / definition 三类操作分开提交)
- Create: 「多选 File Config → 创建 Composition」最小交互(design §11 第 3 条;完整管理界面为非目标)

**Interfaces — Consumes**: T08 的 16 条命令 + T01 的 TS 类型。

**验证**:

- 前端类型检查 + 构建绿;profiles 页手动冒烟:导入/创建/激活/重排/编辑文件/删除(含被引用删除的错误 toast)/更新订阅
- 全仓 `patch_profiles_config` / `chain` 字段引用零残留(grep 前端源码)

**单独 plan 时读**: design §11;guide §7.2/§7.3(TS 破坏性变更表);现 profiles 页组件树。

**2026-07-06 契约修正(现场盘点)**:

- 前端锚点:`use-profile.ts` 唯一导出 hook 为 `useProfile`(内部消费 `getProfiles`/`viewProfile` + update/drop mutations);其余消费点仍按本卡原话 plan 时全量盘点。
- `postprocessing_output` 前端不感知:T07 映射保持 `IRuntime` 对外 shape,`get_postprocessing_output` 返回类型不变,本卡无此项工作。

---

### T10 — legacy 清算(切换组终点)

**目标**: 编译期保证 legacy profiles 面零残留。

**Files**:

- Delete: `backend/tauri/src/config/profile/**` 全目录
- Delete: `backend/tauri/src/core/tasks/jobs/profiles.rs`(`ProfilesJob`/`ProfilesJobGuard`)及其注册点
- Modify: `backend/tauri/src/config/core.rs:42`(删 `Config::profiles()` accessor + `ManagedState<Profiles>` 字段)
- Modify: `backend/tauri/src/client/mod.rs:80`(删旧 `patch_profiles_config` 方法)
- Modify: `backend/tauri/src/feat.rs`(删 `feat::update_profile` 本体)
- Modify: `backend/tauri/src/enhance/mod.rs`(删旧 `enhance()` 函数与 legacy chain 解析;`chain.rs` 中仅被旧路径使用的部分一并清理,builtin 表保留给 T06 适配器)

**验证**(design §16 判据 1):

- `grep -rn "Config::profiles()" backend/tauri/src` 零命中
- `grep -rn "config::profile::" backend/tauri/src` 零命中(新代码只 import `nyanpasu_config::profile::`)
- `ProfilesBuilder` / `ProfileBuilder` / `ProfilesJobGuard` 编译期零引用
- `cargo build` + `cargo test` 全绿

**单独 plan 时读**: design §14 T3.8、§16;T08/T09 完成后的实际残留清单(plan 时以 grep 现场盘点为准,不硬编码本卡文件列表)。

**2026-07-06 契约修正(现场盘点)**:

- 删「`client/mod.rs:80` 旧 `patch_profiles_config` 方法」条——该方法不存在(规划期误记);命令与 helper 已在 T08 删除,本卡仅 grep 核对零残留。
- 增补删除面:`enhance/utils.rs`——全仓唯一 `crate::config::profile` 直接 import(`utils.rs:5`),随旧 `enhance()` 清理一并处理。
- 切换面基数实测(2026-07-06):`Config::profiles()` 共 24 处(ipc 18 / feat 3 / enhance 1 / jobs 2)——T08 清 ipc 后本卡清余下 6 处;grep 判据不变。

---

### T11 — 端到端验证 + 文档收尾

**目标**: 全链路验证 + 迁移账本更新,PR 提交就绪。

**内容**:

1. e2e 冒烟(design §16 判据 2):真实旧 `profiles.yaml` 样本 → migrate 子进程 → `.bak` 在位 → 应用启动 → 激活 profile → `clash-config.yaml` 生成 → 核心可运行;
2. 前端全功能冒烟复核(判据 8);
3. TODO 台账核对(判据 7):`grep -rn "TODO(actor-migration)" backend/tauri/src` 输出与 design §8/D12 清单一致;
4. 文档:`docs/design/actor-migration-roadmap.md` §2.1 状态行更新(PR-3 → 已实施)、§5 台账 B8 登记状态;guide 状态行标注「已实施」。

**验证**: `cargo build && cargo test` + 前端构建全绿;判据 1–8 逐条勾选留痕(PR 描述引用)。

**2026-07-06 契约修正(现场盘点)**:

- e2e 迁移路径:存在双迁移机制(`lib.rs:120` 子进程 + `setup.rs:22-26` in-process runner),plan 时核实 rev3 实际生效路径,e2e 步骤以实测为准;`.bak` 由 CLEAN_SCHEMA step 写入(design §10 安全行)。

---

## 4. 原子切换组说明(T07–T10)

- **T07 之前**: 每个任务 add-only 或纯 migration step,应用行为不变,任意顺序可独立合入 commit。
- **T07–T10 之间为 BC 中间态**:
  - T07 落地后,运行配置生成链路已切换,但旧 IPC 命令仍消费 legacy 类型——**若此时真实运行且 profiles.yaml 已被 revision 3 迁移,旧命令失读**。因此本组期间只要求「编译 + 测试绿」,不要求应用端到端可运行;
  - T08 落地后前端类型检查红,直至 T09 完成——这是铁律 3(前端 BC 同 PR)的预期形态;
  - **本组四卡必须在同一 PR 内连续完成后再请求 review/merge**,不得单独合入 main。
- **T10 之后**: 应用恢复端到端可运行,进入 T11 验证。
- **2026-07-06 叙事修正**:「T07 之前应用行为不变」仅对空数据/新装成立——T02 落地后,真实旧数据启动即被 rev3 迁移(`detect_baseline` 遇 legacy 文件返 0),legacy 读取失效,BC 中间态(真实数据可运行性)实际自 T02 开始。整分支单 PR 合入前提下无实害;开发者中途跑真机需知。

## 5. 执行建议

1. **逐卡出 plan**: 每张卡以「本卡 + design.md 对应章节 + 卡内 Interfaces 契约」为输入,用 `superpowers:writing-plans` 展开为 bite-sized plan(TDD、每步一动作、含完整代码);卡与卡之间只通过 Interfaces 契约耦合,plan 之间不需要互读。
2. **推荐排程**: 先并行 T01/T02/T03(+ T06 若 PR-3-pre② 已合),再 T04→T05,最后一口气完成切换组 T07–T10 + T11。(2026-07-06 增补:T01–T06 已执行完毕;T06A 加固卡排 T07 前,不入原子组。)
3. **契约变更规则**: 实施中若需改动任务卡 Produces 签名,先改本文件对应卡(及下游 Consumes),再改代码——本文件是跨卡契约的唯一权威。

# 配置备份：迁移前自动备份与手动备份

**日期：** 2026-10-01
**状态：** 待实施
**Issue：** [#5527](https://github.com/libnyanpasu/clash-nyanpasu/issues/5527)

## 背景

启动时，`lib.rs` 在初始化日志前以子进程运行 `clash-nyanpasu migrate`（`utils::init::run_pending_migrations`），输出写入 `<data>/migration.log`；`setup.rs` 随后在进程内再次执行 `Runner::run_pending`，正常情况下这一轮已无待执行步骤。

当前迁移链与 1.6 分支差异很大，缺少久经生产运行的配置样本做迁移测试。迁移失败时，runner 只回滚失败的那一步（12 个步骤中只有 6 个实现了 `rollback`），之前已完成的步骤和 `migration-state.yaml` 都保留新状态；用户看到的只是 `panic_dialog` 加 `migration.log`，没有迁移前的文件副本可恢复。

## 目标

1. 有步骤待执行时，在第一个步骤运行前，把迁移可能改写的文件完整快照到 `<data>/backups/`。
2. 迁移失败时保留备份，对话框给出错误、备份位置，并允许直接打开备份目录，引导用户手动恢复。
3. 迁移成功后只保留最新 3 份迁移备份；手动备份同样只保留最新 3 份。
4. 备份逻辑抽成独立的服务，迁移和 `NyanpasuClient` 共用；设置页经由门面手动创建备份、打开备份目录。

## 非目标

- 不做自动恢复，也不在设置页提供「从备份恢复」。运行中覆盖配置文件会与各 actor 持有的状态冲突，失败后用户也可能已经改过文件；恢复由用户在退出应用后手动完成。
- 不备份 `jobs.redb`、`logs/`、`cache/`、`scripts/` 和 sidecar 等非配置数据。
- 不改变 runner 的步骤语义、状态文件格式和 stamp 校验。
- 不做备份列表或管理页面；设置页只提供「立即备份」和「打开备份目录」两个入口。

## 备份范围

| 来源                      | 内容                                                                                                                                                     | 原因                                                             |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| `app_config_dir` 整个目录 | `nyanpasu-config.yaml`、`application.yaml`、`clash-config.yaml`、`session-state.yaml`、`profiles.yaml`、`profiles/`、`migration-state.yaml`、`icons/` 等 | 迁移模块改写的文件都在这里；整目录复制，新增模块无需维护文件清单 |
| `app_data_dir/storage.db` | 键值存储                                                                                                                                                 | `storage` 模块会读写它                                           |

- 若 `app_data_dir` 与 `app_config_dir` 相同或位于其下（例如便携模式），复制配置目录时排除 `backups/`，避免把备份复制进备份；同时排除 `storage.db`、`jobs.redb`、`logs/`、`cache/`、`scripts/` 和 `clash.pid`。它们属于数据目录而非配置，`storage.db` 由单独的来源处理，其余是非目标中明确不备份的运行期数据；`storage.db` 与 `jobs.redb` 在应用运行时还被 redb 锁住，直接复制会在 Windows 上失败。
- 符号链接不跟随，也不在备份内重建（Windows 未开开发者模式时无法创建）；只把链接路径与目标写入 manifest。本地 profile 指向用户自己的外部文件，这些文件迁移不会改写。
- `storage.db` 的获取方式取决于调用时机，见下文「两种调用场景」。

## 目录布局

```text
<app_data_dir>/backups/
  migration-20261001T083015Z-1.6.1-to-2.0.0/
    manifest.json
    config/            # app_config_dir 的副本
    data/storage.db    # 存在时才有
  manual-20261005T120000Z/
    manifest.json
    config/
    data/storage.db
```

- 迁移备份：`migration-<UTC 时间>-<from>-to-<target>`。`from` 取 `migration-state.yaml` 中的 `last_succeeded`，没有时用 `unknown`。
- 手动备份：`manual-<UTC 时间>`。
- 若目标目录名已存在（同一秒内两次备份），追加 `-1`、`-2` 后缀。名字通过原子创建对应的 `.partial` 目录来占用（`create_dir` 遇到已存在即换下一个后缀），因此并发的两次备份不会选中同一个名字。
- 备份先写到同级 `.<name>.partial/`，全部复制完成并写入 manifest 后再 rename 为正式名；失败时删除 `.partial`。每次创建备份前先清掉残留的 `.partial` 目录，所以 `backups/` 下的正式目录都是完整备份。
- `manifest.json`：

```json
{
  "kind": "migration",
  "created_at": "2026-10-01T08:30:15Z",
  "app_version": "2.0.0",
  "from_version": "1.6.1",
  "target_version": "2.0.0",
  "sources": {
    "config": "<app_config_dir 绝对路径>",
    "data": "<app_data_dir 绝对路径>"
  },
  "symlinks": [
    { "path": "config/profiles/xxx.yaml", "target": "D:/my/profile.yaml" }
  ]
}
```

手动备份的 `kind` 为 `manual`，并且没有 `from_version` / `target_version` 两项。迁移备份在 `migration-state.yaml` 没有 `last_succeeded` 时，`from_version` 写 `"unknown"`，与目录名一致。

## 备份服务

备份是无状态的文件系统操作，不持有长期状态，也没有后台任务，因此不做成 actor。它放在迁移模块之外的 `backend/tauri/src/core/backup.rs`，所有依赖都由参数显式传入：

```rust
pub enum BackupKind<'a> {
    Migration { from: Option<&'a Version>, target: &'a Version },
    Manual,
}

/// `storage.db` 的来源。
pub enum StorageSource<'a> {
    /// 没有进程打开该文件（迁移子进程）：直接复制文件。
    File(&'a Path),
    /// 应用运行中，文件被 redb 持有：从同一个只读事务导出一份新库。
    Live(&'a Storage),
}

pub struct BackupRequest<'a> {
    pub paths: &'a PathResolver,
    pub storage: StorageSource<'a>,
    pub kind: BackupKind<'a>,
    pub now: OffsetDateTime, // 由调用方传入，便于测试
}

pub struct BackupInfo {
    pub name: String,
    pub path: PathBuf,
}

/// 创建完整备份；失败时不留下正式目录或 `.partial`。
pub fn create_backup(req: &BackupRequest<'_>) -> Result<BackupInfo, BackupError>;

/// 删除残留 `.partial`，并只保留以 `prefix` 开头的最新 `keep` 份备份。
pub fn prune_backups(backups_dir: &Path, prefix: &str, keep: usize) -> Result<(), BackupError>;
```

- `PathResolver` 新增 `backups_dir()`，返回 `<data>/backups`，与其它 data 派生路径放在一起。
- `Storage` 新增 `export_to(&self, dest: &Path)`：在一个 `begin_read` 事务里遍历 `NYANPASU_TABLE`，写入 `dest` 处新建的 redb 库。这样得到的是一致的快照，也避开了 Windows 上 redb 持有文件锁时直接复制失败的问题。
- `BackupError` 用 `thiserror`，区分 `Io { path, source }`、`Storage` 和 `Manifest`。
- 时间戳用已有依赖 `time` 0.3（启用了 `formatting`）格式化，不新增依赖。

## 两种调用场景

### 迁移前自动备份

由 `Runner` 在执行第一个 `Pending` 步骤前调用 `create_backup`（`StorageSource::File`，`BackupKind::Migration`），因此三个入口都覆盖到：启动子进程、`setup.rs` 的进程内二次运行，以及 CLI 的 `migrate --migration <id>` / `--force`。

- `run_pending`：先按 `advice_step` 收集全部步骤；有 `Pending` 时先备份再进入循环，没有就跳过。
- `run_migration`：advice 为 `Pending` 时先备份。
- 一次 `Runner` 生命周期最多备份一次（字段 `backup: Option<BackupInfo>` 记录结果），并在 `migration.log` 中打印 `Backup created at <path>`。
- 全新安装：`reconcile_documents` 只为文档模块把缺失文件记为 head，`typed_config` 在空目录上基线为 revision 0，所以它的步骤仍是 `Pending`。但此时没有东西需要保护，`ensure_backup` 在配置目录中除 `migration-state.yaml` 外没有任何条目、且 `storage.db` 不存在时跳过备份，只在 `migration.log` 中记录一行。其余情况照常备份。
- 备份失败（磁盘满、权限不足等）时中止迁移并返回错误，绝不在没有备份的情况下迁移。
- 迁移在子进程里进行，此时 GUI 尚未启动，`storage.db` 没有被其它进程打开，可以直接复制。`setup.rs` 的二次运行发生在 `core::storage::setup` 打开存储之前，同样直接复制。

`Runner::with_context` 中的 `ensure_baselines` 可能在备份前写一次 `migration-state.yaml`。这次写入只记录与磁盘文件一致的 baseline，不改写配置文件，所以备份里的状态文件与配置文件仍然一致，恢复后下次启动可以从同一个起点重新迁移。实现时不调整这个顺序。

### 设置页手动备份

手动备份经由 `NyanpasuClient` 门面提供：

```rust
impl NyanpasuClient {
    /// 创建一份手动备份，并只保留最新 `KEEP_MANUAL_BACKUPS` 份 `manual-*`。
    pub async fn create_config_backup(&self) -> Result<BackupInfo, BackupError>;
    /// `<data>/backups`，不存在时创建。
    pub fn backups_dir(&self) -> std::io::Result<PathBuf>;
}
```

- 门面已持有 `PathResolver`（`ClientSetupArgs.paths`）；新增 `ClientSetupArgs.storage: Storage`，由门面保存。
- `Storage` 改由组合根打开：在 `setup.rs` 中构建 client 之前用 `paths.storage_path()` 调 `Storage::try_new`，打开失败直接返回错误；把同一个实例传给 client，并 `app.manage(storage.clone())` 供现有 storage 系列命令和 `RpcDependencies` 使用。随后删除 `resolve_setup` 里的 `core::storage::setup(app)` 以及读全局 `dirs::storage_path()` 的 `core::storage::setup` 函数。目前该调用只是 `log_err!`，但打开失败时 `setup_unified_rpc` 里的 `app.state::<Storage>()` 也会 panic，所以把失败改为启动错误并不改变实际结果。
- `create_config_backup` 内部用 `tokio::task::spawn_blocking` 调 `core::backup::create_backup`（`StorageSource::Live`、`BackupKind::Manual`），成功后调 `prune_backups(backups_dir, "manual-", KEEP_MANUAL_BACKUPS)`，其中 `KEEP_MANUAL_BACKUPS = 3`；清理失败只记录警告，不影响本次备份成功。任务 panic 时 `resume_unwind`，不把 `JoinError` 转成普通错误。
- 不加锁：前端用 `useLockFn` 防止重复点击；即使两次请求并发，目录名后缀也能保证互不覆盖，清理只删除已完成的旧目录。
- 运行中备份的一致性：各 actor 都用原子 rename 写配置文件，所以每个文件单独看都是完整的某个版本，但多个文件之间不保证是同一时刻的快照；profile 服务的 `staging/`、`journal/` 会随目录一并复制，恢复后由它自己在启动时收尾。用户手动备份可以接受这一点，界面上不额外提示。

新增两个 RPC 命令，都是 mutation，都只在桌面端可用（不加 `http`），在 `specta_export.rs` 中与 `open_app_data_dir` 等命令登记在一起。命令只做适配：

| 命令                   | 签名                                                                      | 行为                                                               |
| ---------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `create_config_backup` | `async fn(client: State<'_, NyanpasuClient>) -> Result<ConfigBackupInfo>` | 调 `client.create_config_backup()`，返回备份名与路径（specta DTO） |
| `open_backups_dir`     | `fn(client: State<'_, NyanpasuClient>) -> Result<()>`                     | `utils::open::that(client.backups_dir()?)`                         |

前端在 `settings/debug/_modules/path-utils-card.tsx` 现有的目录按钮旁增加两个按钮：

- 「打开备份目录」：调用 `rpc.openBackupsDir()`。
- 「立即备份」：用 `useLockFn` 防止重复点击，调用 `rpc.createConfigBackup()`，成功时用页面现有的通知方式显示备份名，失败时显示错误。
- 新增 paraglide 文案键（`settings_debug_utils_open_backup_directory`、`settings_debug_utils_create_backup` 及成功/失败提示），添加后需执行 paraglide compile。

## 失败处理与对话框

子进程失败时，父进程（`lib.rs`）不再调用 `panic_dialog`，改为新增的 `utils::dialog::migration_failed_dialog`，使用 rfd 0.15 的 `MessageButtons::YesNoCancelCustom` 提供三个按钮：

| 按钮         | 行为                                                          |
| ------------ | ------------------------------------------------------------- |
| 打开备份目录 | `utils::open::that(<data>/backups)`，然后重新显示对话框       |
| 打开迁移日志 | `utils::open::that(<data>/migration.log)`，然后重新显示对话框 |
| 退出         | `exit(1)`                                                     |

- `migration_failed_dialog(error, &PathResolver, backup_failed)` 在用户选择「退出」后返回，调用方随后 `exit(1)`；直接关闭对话框等同于「退出」。若 `PathResolver::from_env` 失败，`lib.rs` 退回到原来的 `panic_dialog`。
- 子进程退出状态由 `utils::init::MigrationChildFailed { status, stderr }` 携带，`lib.rs` 对 `anyhow::Error` 做 `downcast_ref` 读取退出码。
- 父进程只需要知道 `<data>/backups` 这个固定位置，不必从子进程拿具体目录名；对话框正文提示「最新的 `migration-*` 目录即本次迁移前的备份」。
- 不再在失败时自动打开 `migration.log`，改为用户按需点击。
- 正文包含：错误摘要、备份目录路径、恢复步骤（退出应用 → 用备份中的 `config/` 覆盖配置目录、`data/storage.db` 覆盖数据目录中的同名文件 → 安装迁移前的版本，或附上 `migration.log` 提交 issue）。
- 备份本身失败时，对话框说明配置文件未被修改，改用 `OkCancelCustom`，只提供「打开迁移日志」和「退出」两个按钮，不显示「打开备份目录」。父进程靠子进程退出码区分：备份失败为 `2`，其它失败仍为 `1`。`cmds::migrate::parse` 对 `anyhow::Error` 做 `downcast_ref::<BackupError>()` 来选择退出码。
- 文案走 `rust_i18n`，新增 `dialog.migration_failed.*` 键，五个 locale（en / zh-cn / zh-tw / ru / ko）同步添加。
- `setup.rs` 进程内运行失败的现有路径不变：它返回错误后由现有错误处理收尾。若这一轮实际执行了步骤，它也会先备份。

## 成功后的清理

迁移报告成功，并不代表产出的配置一定正确。因此成功后保留本次备份，并调用 `prune_backups(backups_dir, "migration-", KEEP_MIGRATION_BACKUPS)`，其中 `KEEP_MIGRATION_BACKUPS = 3`。只有出现待执行迁移时才会产生备份，通常每次升级一份，占用可控。

- 「最新」按名字中的 UTC 时间戳排序，时间戳相同时依次比较目录修改时间和名字（同一秒内的 `-1`、`-2` 备份不能只靠名字排序）。
- 清理在 `run_pending` 成功并 flush 状态之后进行；清理失败只打印警告，不影响迁移成功的结果。
- 迁移失败时不做任何清理。
- 手动备份（`manual-` 前缀）不受影响，它们在每次手动备份后单独清理，同样保留 3 份。

## 测试

全部使用 `tempfile` 构造的 `PathResolver::with_base_dirs`，不触碰真实用户目录。

1. `create_backup` 复制配置目录与 `storage.db`，manifest 字段正确；`storage.db` 不存在时不报错。
2. data 目录位于 config 目录下时，备份中不包含 `backups/`。
3. 符号链接（仅 unix 测试，或 Windows 下检测到无权限时跳过）记录进 manifest，并且不被跟随。
4. 复制中途失败（例如注入一个不可读文件）时不留下正式目录，也不留下 `.partial`。
5. 同一 `now` 连续创建两次，第二份带 `-1` 后缀。
6. `StorageSource::Live`：在打开的 `Storage` 上写入若干键后导出，导出库可以用 `Storage::try_new` 打开，且内容一致；原库保持可用。
7. `prune_backups` 只保留对应前缀的最新 N 份，删除 `.partial` 残留，不动其它前缀和无关条目。
8. Runner：
   - 无待执行步骤时不创建备份；
   - 全新安装（空配置目录）执行其待执行步骤但不创建备份；
   - 有待执行步骤时恰好创建一份，且备份内容是迁移前的文件（用 fixtures 中的旧配置验证）；
   - 某一步失败时备份保留、不执行清理；
   - 成功后 `migration-*` 只剩 3 份，`manual-*` 不受影响；
   - 备份失败时不执行任何步骤，配置文件与状态文件均未改动（`ensure_baselines` 的写入除外）。
9. 门面：用测试 `NyanpasuClient`（临时目录 + 临时 `Storage`）连续调用 `create_config_backup` 4 次，`manual-*` 只剩最新 3 份，`migration-*` 不受影响。
10. Windows 手动冒烟：

- 放入一份会使迁移失败的配置，确认三个按钮的行为、文案以及退出码 2 的分支；
- 应用运行中在设置页点「立即备份」，确认生成 `manual-*` 且 `storage.db` 可打开；「打开备份目录」能打开正确位置。

## 实施步骤

1. `PathResolver::backups_dir`、`Storage::export_to`、`core::backup`（`create_backup` / `prune_backups`）及单元测试 → 验证：测试 1–7 通过。
2. Runner 接入迁移前备份与成功后清理 → 验证：测试 8 通过，现有 migration 测试不回归。
3. `cmds::migrate` 退出码、`migration_failed_dialog`、`lib.rs` 替换 `panic_dialog`、`dialog.migration_failed.*` 文案 → 验证：clippy 通过，冒烟第一项。
4. 组合根打开 `Storage` 并注入 `NyanpasuClient`，删除 `core::storage::setup`；门面新增 `create_config_backup` / `backups_dir` → 验证：测试 9 通过，现有 client 测试不回归。
5. `create_config_backup` / `open_backups_dir` 命令与元数据登记、重新生成绑定、设置页两个按钮与 paraglide 文案 → 验证：clippy、`pnpm typecheck`、`pnpm lint` 通过，冒烟第二项。

每步一个原子提交。

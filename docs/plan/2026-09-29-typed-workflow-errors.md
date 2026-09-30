# Workflow 错误强类型化实施计划

> **状态**：已实施（2026-09-29），分 PR-1..PR-6 叠加交付，待评审。
>
> **基线**：`origin/main@62b85326c`，`backend/nyanpasu-runtime@79be37e4b`。
>
> **行号约定**：路径未加前缀时相对 `backend/tauri/src/`，行号为基线行号，实施时按符号重新定位。

## 1. 目标与用户裁定

- **目标**：后端主体 workflow 的大多数失败以领域错误到达前端，前端按错误种类本地化（paraglide）。能不用 anyhow 的地方都不用。
- **裁定 1（错误建模）**：错误偏向领域；特定错误携带上下文。
- **裁定 2（展示）**：用户只看到最简单的信息；弹窗上有一个按钮，复制原始错误，供开发者诊断。
- **裁定 3（范围）**：profiles、runtime/core、配置 patch、系统代理/DNS/热键/widget 四个领域全部纳入。
- **裁定 4（交付）**：叠加 PR，先线缆契约，再逐领域。

## 2. 现状（摸底结论）

- 所有命令返回 `IpcError`（`ipc.rs:25`）。它的 `Serialize` 是 `format!("{self:#?}")`（`ipc.rs:69-76`），`specta::Type` 手写成 `string`（`ipc.rs:78-83`）。前端只能拿到一段 Debug 文本，约 40 处经 `formatError` 原样显示。
- 类型在进入 IPC 之前已经丢失：
  - `ClientError::Anyhow` / `IpcError::Anyhow` / `Custom(String)` 兜底；
  - `client_error_from_core`（`client/mod.rs:130`）把 `CoreError` 包成 anyhow；
  - `application_workflow` 的 port trait 返回 `anyhow::Result`；
  - `Ack::Failed(anyhow::Error)`（`backend/nyanpasu-core/src/state/ack.rs:39`）；
  - `ProfilesError` 有 5 个只带 `String` 的变体；`Degradation.code` 是自由字符串。
- 可直接复用：`ProfilesError`、`CoreErrorKind`（runtime 的 `nyanpasu-core-metadata`，已派生 specta + serde）、`RuntimePipelineError`、`ProfileValidationError`。
- 前端先例：日志命令的 `LogError`（`typedError<_, LogError>`，按种类 `switch` 到 `m.logs_*()`），不走 `IpcError`，本计划不动它。

## 3. 线缆契约（PR-1）

```ts
type IpcError = {
  kind: IpcErrorKind // 领域错误，前端据此本地化
  message: string    // 错误自身的 Display，kind 无法本地化时显示
  detail: string     // 原始错误（Debug，含 source 链），仅供“复制错误详情”
}
type IpcErrorKind = { domain: 'unknown' } | { domain: 'profiles'; error: ProfilesError } | ...
```

- Rust 侧 `IpcError` 改为上述结构体，派生 `Serialize` + `specta::Type`。
- 转换只有一个泛型入口：任何 `E: Display + Debug` 且 `IpcErrorKind: From<E>` 的错误都能经 `?` 变成 `IpcError`，构造时取 `message = e.to_string()`、`detail = format!("{e:?}")`，再把 `e` 移入 `kind`。
- `IpcErrorKind` 用相邻标签（`domain` + `error`）。各领域错误自身用内部标签（`kind`）+ 结构体变体携带上下文。
- 领域错误里的第三方错误（io、reqwest、serde…）标注 `#[source]` 并 `#[serde(skip)]`：它们只通过 `detail` 到达前端。
- 领域变体在 `IpcErrorKind` 中装箱（`Box<..>`），避免命令返回值触发 `result_large_err`；线缆形状不变。
- PR-1 只有 `unknown` 一个领域；其余领域在各自 PR 中加入。PR-1 期间用户看到的是错误的 Display（不再是 Debug 文本），不比现状差。

## 4. 前端（PR-1）

- `@nyanpasu/interface`：`isIpcError(value)` 类型守卫。
- `@nyanpasu/nyanpasu`：
  - `formatError(err)` 返回最简单的信息：`IpcError` → 本地化 `kind`，无法本地化时用 `message`；`Error` → `message`；其他 → `String(err)`。去掉原来的 `Error: ` 前缀。
  - `message(text, { kind: 'error', error })`：`error` 是 `IpcError` 时弹窗带“复制错误详情”与“关闭”两个按钮，点复制把 `detail` 写入剪贴板。原生弹窗的自定义按钮在三个平台都返回按钮文字（tauri-plugin-dialog 2.8.0 `desktop.rs:227-237`）。
  - 所有把命令错误 `String(...)` 的地方改用 `formatError`。

## 5. 建模规则（PR-2 起适用）

采用 snafu 的思路，并直接使用 snafu（0.9.2，已在锁文件里，经 bolt-load 间接引入）：

1. **变体按“当时在做什么”命名，而不是按失败的库命名。** 同一种 source（如 `io::Error`）可以出现在多个变体里：`ReadProfile { path, source }`、`WriteProfile { path, source }`，不要 `Io(#[from] io::Error)`。
2. **上下文在失败点通过 context selector 附加**：`fs::read(&path).context(ReadProfileSnafu { path })?`、`ensure!(..., ProfileInUseSnafu { id })`。不再用 `.context("...")` 字符串，也不写 `map_err` 闭包。
3. **上下文字段即前端文案参数**：profile id/名称、URL、HTTP 状态、路径、端口、内核类型等可序列化；`source` 标 `#[serde(skip)]`，只经 `detail` 到达前端。
4. **每个领域一个模块级错误枚举**，派生 `Snafu + Serialize + specta::Type`，`#[serde(tag = "kind")]`；selector 用 `#[snafu(visibility(pub(crate)))]` 限定可见性。
5. port trait 与服务方法签名不返回 `anyhow`；`unknown` 只留给本计划范围外的命令。
6. 同一失败只在一个地方分类；上层用一个带上下文的变体包住下层领域错误（`source` 为下层枚举），不再包字符串。

**渐进迁移**：snafu 与 thiserror 都实现 `std::error::Error`，可以共存。只在领域 PR 触及时把该领域的 thiserror 枚举改写为 snafu；未触及的保持原样。

## 6. PR 栈

| PR   | 分支                               | 内容                                                                                                                                          |
| ---- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| PR-1 | `refactor/typed-errors-1-wire`     | 本计划；线缆契约 + 前端框架（§3–§4）                                                                                                          |
| PR-2 | `refactor/typed-errors-2-profiles` | 共享叶子 `NotReady` / `CommitAborted`（取代 `uncommitted()` 文本）；profiles 端口、`profile_file.rs`、`ProfilesError` 改 snafu；域 `profiles` |
| PR-3 | `refactor/typed-errors-3-runtime`  | `RuntimeError` + `CoreFailure`（不再把 `CoreError` 抹成 anyhow）；构建/发布/端口/内核二进制/服务命令/运行时读取；域 `runtime`                 |
| PR-4 | `refactor/typed-errors-4-acks`     | `Ack` 载荷改为 `AckError`（`Arc<dyn Error>`，在 `CommitAborted::classify` 唯一一次 downcast）；运行时拒绝类型化；`Degradation.reason` 枚举    |
| PR-5 | `refactor/typed-errors-5-config`   | 应用/Clash 配置、运行时覆盖、会话状态的 `ConfigError`；`StorageOperationError`；`HotkeyParseError` 前端本地化；域 `config`、`storage`         |
| PR-6 | `refactor/typed-errors-6-effects`  | 系统 DNS、系统代理/PAC/自启、热键注册、widget、托盘/日志刷新、效果重试；`EffectFailureCode`；域 `system_dns`、`system_proxy`、`effects`       |

每个领域 PR 均先盘点经 IPC 可达的失败点再定变体清单；每个提交可独立构建，均通过 workspace clippy、`clash-nyanpasu` 全量 lib 测试、bindings 导出、`pnpm lint:ts` 与前端测试。

## 7. 未覆盖（后续）

- 启动/重建路径的原因文本：`RecoveryUnresolved`、`CoreNotStarted` 的 `reason`，以及 `startup.rs` 约 15 处 `format!` 原因。
- 状态面（调试页）的文本字段：`RetryableCause.message`、`DeferredTarget.cause`、`RecoveryView.reason`、`MutationReceipt.detail`。
- nyanpasu-core `replace_if_version` 的写入/恢复闭包仍为 anyhow，promote 失败的具体原因只经 `detail` 到达前端。
- 内核更新器（`UpdaterError`）、启动期 `setup.rs`、迁移模块不在本计划范围。
- 配置字段没有校验器；Tauri 参数反序列化失败仍是纯字符串。
- `cfg(unix)` / `cfg(macos)` 分支只在 Windows 以外由 CI 编译，本地仅人工审阅。

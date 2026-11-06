# Core 包族拆分、Shell 与 RPC 目标 任务

关联设计：`design.md`（同目录）。路径缩写同 design §1。

## 0. 全局约束

1. 遵守 `AGENTS.md` 与 `docs/development/*`：不加全局单例；依赖显式注入；不给 core 族加 Tauri 依赖；`NyanpasuClient` 只是 facade。
2. 只移动代码、注入依赖、收窄依赖方向。行为改动单独成卡（A2b、C1、C2），不夹在移动提交里；移动中发现的现存问题记在 PR 描述里，不顺手改。
3. 每个提交可编译、测试通过、只做一件事；调用方在同一提交迁完，不留 re-export、别名或兼容层（design §7）。
4. 每个新 crate 的提交同时更新：workspace `members` 与依赖项（指向 config、core 的边关默认 feature）、门禁集合与策略测试（design §3.2）、`STATIC_GATE_PREFIXES` 与覆盖测试、静态允许名单路径（design §4.6）。
5. 被移走、但仍被原 crate 测试使用的夹具与 `pub(crate)` 项，同提交改为公开（测试夹具放在新 crate 的 `test-support` feature 后），原 crate 以 dev-dependency 开启该 feature，同时在原 crate 的 `test-support` 中转发给下游。
6. Rust 代码按 `docs/development/rust.md`：新错误类型用模块自有 snafu 枚举、变体按失败步骤命名；新端口 trait 由消费者定义、可 `mockall::automock`；构造函数用 `new`/builder。
7. 每张卡的本地检查：`cargo clippy --workspace --all-targets --all-features`；动到的包的 `cargo test`；`pnpm run lint:backend-boundaries`；`pnpm run lint:architecture-ledger`；改了脚本则 `pnpm run test:scripts`；移动 specta 类型则重新导出 bindings，三个生成文件无 diff。动到 `cfg(unix)`/`cfg(target_os = "macos")` 代码时补 `cargo check --target x86_64-unknown-linux-gnu`。
8. 检查方法：`rg` 的旧路径检查限定在所属 crate，退出码 1 才算无匹配，退出码 2 是检查失败；"测试原样通过"用移动前后的测试名清单比较：先把 `cargo test -p <包> -- --list --format terse` 的输出写入文件并检查 cargo 退出码为 0、stderr 保留，清单为空视为失败，再排序比较；只允许文档化的模块前缀变化；不比较行数；fixture 未变用 `git hash-object` 比较。
9. 开工前按 `docs/development/workflow.md` 评估 worktree 与当前检出并询问用户；Phase A 动 core 与 tauri，推荐 worktree。

## 1. Phase A 的 PR 划分

| PR    | 卡                    | 依赖       | 说明                             |
| ----- | --------------------- | ---------- | -------------------------------- |
| PR-T0 | T0a、T0b、T0c         | —          | 门禁表化、feature 接线、基线测量 |
| PR-A1 | A1a、A1b、A1c         | T0         | 三个独立提交，同一 PR            |
| PR-A2 | A2                    | A1b        | 可与 A3、A4 并行                 |
| PR-A3 | A3                    | A1         | 可与 A2、A4 并行                 |
| PR-A4 | A4a、A4b              | A1c        | 两个提交                         |
| PR-A5 | A5a → A5b → A5c → A5d | A2、A3、A4 | 叠加栈，每个提交独立可编译       |

A5 各卡按 design §8.2 已定的 O-1 a（`nyanpasu-persistence`）与 O-2 c（订阅 HTTP 进 net）写。

---

## T0a — 门禁改为规则表

- [x] `S/backend-boundaries.ts`：用具名集合（`GUI`、`FOUNDATION`、`CORE_FAMILY`、`IMPLEMENTATION`）加 `{ package, forbids }` 规则表替换 `NEUTRAL_PACKAGES` 与 `dependencyViolations` 里的 `if` 链；检查某包时跳过 `cargo tree` 输出中的根包自身；`cargo tree` 参数不变。
- [x] 本卡只表达现有规则：GUI 族对所有中立包、core → platform、diagnose → core/platform。集合中尚不存在的包不出现在表里。
- [x] `S/backend-boundaries_test.ts`：每条规则一正一反；新增"根包自身不算违规"与"禁止 `nyanpasu-local-channel` 拦住 widget `ipc` feature"的用例；保留"platform → core 允许"的现有用例直到 A5d。

**验证**

- `pnpm run test:scripts` 通过；`pnpm run lint:backend-boundaries` 在本分支通过。
- 反向检查：临时删掉规则表里 core → platform 那行，对应策略测试变红。

## T0b — feature 接线

- [x] 以下每条边加 `default-features = false` 并显式列出所需 feature：tauri → core（`backend/tauri/Cargo.toml:39`，dev `:271` 保留 `test-support`）、platform → core（`backend/nyanpasu-platform/Cargo.toml:16`，dev `:65`）、core → config（`backend/nyanpasu-core/Cargo.toml:77`）、platform → config（`:19`）、tauri → config（`backend/tauri/Cargo.toml:41`）、diagnose → config（`backend/nyanpasu-diagnose/Cargo.toml:10`）。
- [x] tauri 的 `default-meta`、`nesting` 转发（`backend/tauri/Cargo.toml:285-299`）保持语义：tauri 默认 feature 仍打开它们。
- [x] 不改 `test-support` 经 dev-dependency 打开的做法（O-3）。

**验证**

- 修改前后用同一组参数各执行一次 `cargo tree -p clash-nyanpasu -e features -i nyanpasu-core` 与 `-i nyanpasu-config`（默认 feature），两次输出的 feature 集合一致；差异写进 PR 描述。
- `cargo tree -p clash-nyanpasu --no-default-features -e features -i nyanpasu-config` 与 `-i nyanpasu-core` 中都不出现 `default-meta`（同一组参数下检查，修改前该命令会出现）。

## T0c — 构建基线

- [x] 按 design §6 测：干净 `cargo build --timings -p clash-nyanpasu`，四个增量场景各 3 次取中位数；记录环境字段。
- [x] 额外记录 `cargo build -p clash-nyanpasu` 与 `cargo test -p clash-nyanpasu --no-run` 各编译几份 `nyanpasu-core`（`--timings` 报告中的单元），作为 O-3 的数据。
- [x] 结果写进 PR-T0 描述，附总时长、core 单元时长、关键路径。

**验证**

- PR 描述含四个场景的三次原始数据与中位数，环境字段齐全。

---

## A1a — `owner_shutdown` 移到 core 根模块

- [x] `C/client/owner_shutdown.rs` → `C/owner_shutdown.rs`，在 `C/lib.rs` 声明，删除 `C/client/mod.rs:6` 的声明；内容不变。
- [x] 更新全部使用者：`C/client/{app_lifecycle.rs,app_update/mod.rs,application.rs,application_workflow/mod.rs,clash_config.rs,effects/actor.rs,hotkey/mod.rs,profiles.rs,session_state.rs,system_proxy/mod.rs}`、`C/actor_v2/service_actor.rs`、`C/clash/ws.rs`、`C/geo/client.rs`、`C/logs/actor.rs`、`C/proxies.rs`、`C/traffic/client.rs`、`C/updater/mod.rs`、`T/setup.rs:4`。

**验证**

- `rg 'client::owner_shutdown' backend/nyanpasu-core backend/tauri` 退出码 1。
- `cargo test -p nyanpasu-core` 通过，`C/client/app_lifecycle.rs` 的关闭测试在测试名清单中。

## A1b — 删除 `ScriptType`，统一用 `ScriptRuntime`

- [x] 删除 `C/enhance/mod.rs:8-53` 的 `ScriptType`、`ScriptWrapper` 及其测试；`ScriptWrapper` 在 `P/enhance/script/` 内重新定义为私有类型（`ScriptRuntime` 与 `String`），`RunnerManager::process_script` 收窄为模块内可见。
- [x] `P/enhance/script/{adapter.rs,runner.rs}` 改用 `nyanpasu_config::profile::ScriptRuntime`；删除 `P/enhance/script/mod.rs:8` 的 re-export 与 `:12,16` 的注释代码。
- [x] 若 config 没有 `ScriptRuntime::JavaScript` 序列化为 `"javascript"` 的测试，把被删的测试改写为该断言放进 `Q/profile/definition.rs` 的测试；已有则不补。

**验证**

- `rg 'ScriptType' backend/nyanpasu-core backend/nyanpasu-platform` 退出码 1。
- platform 的 script 测试名清单与改动前一致且通过；`cargo test -p nyanpasu-config` 通过；三个生成文件无 diff。

## A1c — 新建 `nyanpasu-contracts`，移入 `BundleMetadata`

- [x] 新建 `backend/nyanpasu-contracts`，依赖只有 `nyanpasu-config`（关默认 feature）；`lib.rs` 只含 `bundle_metadata`。
- [x] `C/client/bundle_metadata.rs` 移入；`TEST_BUNDLE_METADATA` 改为 `#[cfg(feature = "test-support")]`；core 的 `test-support` 转发 `nyanpasu-contracts/test-support`，core 加 dev-dependency `nyanpasu-contracts = { …, features = ["test-support"] }`；platform、tauri 的 dev 依赖同样开启。
- [x] 更新全部使用者：core 的 `client/{mod.rs,app_update/mod.rs,app_update/tests.rs,session_state.rs,startup_recovery_tests.rs,test_support.rs,test_support/facade.rs,test_support/startup_recovery.rs}`、`backup.rs`、`migration/{mod.rs,runner.rs,modules/profiles.rs}`、`service/icon.rs`、`updater/instance.rs`、`utils/{candy.rs,net.rs}`；platform 的 `changelog_http.rs`、`profiles/{fs_service.rs,test_support.rs,fs_service/integration_tests/profiles.rs}`；tauri 的 `adapters/app_update.rs`、`bundle.rs`、`bundle/tests.rs`、`setup.rs`。
- [x] 登记：workspace members；`FOUNDATION` 集合加 contracts，contracts 的附加禁止项（`ractor`、`reqwest`、`redb`、`nyanpasu-core-manager`、`nyanpasu-local-channel`）与策略测试；`STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-contracts/src/` 与覆盖测试。

**验证**

- `rg 'mod bundle_metadata' backend/nyanpasu-core/src/client` 退出码 1（旧位置的模块声明已删）；`rg 'client::bundle_metadata|client::BundleMetadata' backend` 退出码 1（旧限定路径无残留；不查字段名，不禁止 contracts 中的新声明）。
- 单独 `cargo test -p nyanpasu-core` 与 `cargo test -p nyanpasu-platform` 通过（证明 dev 侧 `test-support` 接好）。
- `pnpm run lint:backend-boundaries`、`pnpm run test:scripts`、`pnpm run test:architecture-ledger` 通过；反向检查：给 contracts 临时加 `ractor` 依赖，门禁失败。

---

## A2 — `nyanpasu-scripts` 与 runner 构造闭包

- [x] core `C/client/application_workflow/ports.rs` 增加 `ScriptRunnerFactory` 类型（design §4.2），文档注释写明在阻塞线程上调用、runner 用完即销毁。
- [x] 新建 `backend/nyanpasu-scripts`，`P/enhance/script/` 七个文件整体移入；依赖带上 `backend/nyanpasu-platform/Cargo.toml:20-21,39-55` 中的脚本依赖与这些文件实际用到的共享依赖；依赖只有 config 与这些库。
- [x] `P/enhance/runtime_build.rs`：`FsRuntimeBuildAdapter` 的 `ScriptDirs` 字段改为 `Arc<ScriptRunnerFactory>`，`:82` 改为在阻塞闭包内调用；`StartScriptRunner` 的 source 从 `io::Error` 改为 `PortError`。
- [x] `P/enhance/runtime_build/tests.rs:60-65,89-93` 改为经闭包构造 runner（platform dev-dependency scripts）；golden 与 `backend/nyanpasu-platform/tests/runtime_builder.rs` 同样处理。
- [x] `T/setup.rs:357-371` 构造闭包并注入；tauri 加 scripts 依赖；platform 删除脚本引擎依赖。
- [x] 登记 scripts：workspace、`IMPLEMENTATION` 集合、`STATIC_GATE_PREFIXES`。

**验证**

- scripts 的测试名清单等于移动前 platform 中 `enhance::script` 下的测试名清单（模块前缀不同，名字一致）且全部通过。
- platform 的 golden 7 例、`runtime_build/tests.rs`、`tests/runtime_builder.rs` 通过。
- 门禁通过；反向检查：给 scripts 临时加 core 依赖，门禁失败。

## A3 — `nyanpasu-profile-transform`

- [x] 新建 `backend/nyanpasu-profile-transform`，移入 `C/enhance/{runtime_builder.rs,chain.rs,utils.rs,builtin/}`，`LogSpan` 公开导出；`test_support.rs` 门控为 `#[cfg(any(test, feature = "test-support"))]`，`metadata` 等被 core 测试使用的夹具改为 `pub`。
- [x] `C/enhance/artifact_snapshot.rs` 移到 `C/client/` 下 `runtime.rs` 旁（模块名按内容取），改用 transform 的 `LogSpan`；删除 `C/enhance/` 目录与 `lib.rs` 声明。
- [x] 更新调用方：`C/client/{runtime.rs,runtime_error.rs,application_workflow/ports.rs,runtime_inspection.rs,test_support/runtime_build.rs,application_workflow/impact.rs}`、`C/client/application_workflow/tests/{mod.rs,connection_policy.rs,mutations.rs}`、`C/state/profiles/{sources.rs,actor.rs}` 的测试、`P/enhance/runtime_build.rs`、`P/enhance/runtime_build/{tests.rs,golden.rs}`、`backend/nyanpasu-platform/tests/runtime_builder.rs`、`T/ipc.rs:19`。
- [x] core 加 transform 依赖；core 的 `test-support` 转发 transform 的 `test-support`，并加 dev-dependency 开启它；platform 的 dev 依赖同样开启。
- [x] 登记 transform：workspace、`FOUNDATION` 集合（附加禁止 `nyanpasu-contracts`、`nyanpasu-persistence`、`nyanpasu-scripts`，及 config 不得依赖 transform）与正反策略测试、`STATIC_GATE_PREFIXES`。

**验证**

- `rg 'crate::enhance' backend/nyanpasu-core/src` 退出码 1；`rg 'nyanpasu_core::enhance' backend/nyanpasu-platform backend/tauri` 退出码 1。
- 门禁通过（transform 不依赖 core 族、实现 crate）；三个生成文件无 diff。
- 单独 `cargo test -p nyanpasu-core`、`-p nyanpasu-platform`、`-p nyanpasu-profile-transform` 通过，transform 的测试名清单等于移动前 core 中对应模块的清单。

---

## A4a — contracts：profile 与 materialization 端口

- [x] 改动前，在当前提交上为每个将被替换的错误变体生成期望值（序列化 JSON、`Display`、逐级 `source()` 文本，用真实 reqwest/atomicwrites/serde_yaml/corrupt 错误构造），写成测试，确认通过。
- [x] 移入 `C/state/profiles/ports.rs` 的端口与值类型，以及 `error.rs` 中只属于这些端口的错误（`ProfileFileError`、`SubscriptionFetchError`、`ErrorPath`、`ExpectedNode`）；`ProfilePathError` 留在 config；`ProfilesError` 与事务错误留在 core。
- [x] contracts 定义 `BoxedCause`；`ProfileFileError` 的 `corrupt::Error`、`atomicwrites::Error`、YAML 错误字段与 `SubscriptionFetchError` 的三个 `reqwest::Error` 字段改为 `BoxedCause`，`#[serde(skip)]` 不变；实现方在产生处装箱。
- [x] `ProfileDegradationCode::retryable` 改为 `pub`；`SubscriptionFetchError::mock` 改为 contracts `test-support` 下的公开构造器，`C/client/mod.rs:2974,3134` 等测试改用它；`C/client/test_support.rs:48,784` 用到的 mock 随 trait 由 `automock` 生成并在 `test-support` 下导出。
- [x] 更新全部调用方（core 的 profiles、jobs、client 及测试；platform；tauri `ipc.rs` 测试）。

**验证**

- 第一步写的期望值测试在改动后原样通过（替换后的变体 JSON、`Display`、source 链不变）。
- `T/ipc.rs:2359-2390`、`frontend/nyanpasu/tests/ipc-error.browser.test.ts` 通过；三个生成文件无 diff。
- `backend/nyanpasu-contracts/Cargo.toml` 不声明 `reqwest`、`atomicwrites`、`serde_yaml*`；门禁通过。
- 反向检查：把某处装箱改成 `BoxedCause::from(e.to_string())`（能编译，但丢失下层 source 链），期望值测试变红。

## A4b — contracts：网络端口

- [x] 移入 `ChangelogHttp`（`C/client/app_update/changelog.rs:46-50`）、`SelfProxyPortSource`（`C/service/profile_file/ports.rs:3-8`）、`DirectEgressProbe`（`C/client/direct_egress.rs:25-31`）；`SessionPortResolver`（`C/client/ports.rs:232`）留在 core 实现它。
- [x] 新增三个消费端口：`IconFetcher`（`C/service/icon.rs:35-83`，返回 `bytes::Bytes`）、网络诊断（`C/utils/net.rs` 的 `url_delay_test`、`get_ipsb_asn`，调用方 `C/client/mod.rs:793,803`）、`SystemProxySource`（替代 `C/utils/config.rs:11-19` 的直接调用，实时读取）。现有函数的 `PathResolver` 与元数据改为实现的构造依赖；self proxy 端口仍按每次调用传入（facade 照旧用 `self.clash_info().port`，`C/client/mod.rs:796,803,813`），不在构造时固定。
- [x] 图标抓取、网络诊断的现有实现移到 core 内新模块 `C/net_adapters.rs`（A5c 整体移入 net）；图标服务与 facade 方法改为持有端口；tauri `setup.rs` 构造并注入。
- [x] 更新调用方与测试实现（`T/unified_rpc.rs:863` 等）。

**验证**

- `rg -l 'get_reqwest_client' backend/nyanpasu-core/src` 只列出 `utils/candy.rs` 与 `net_adapters.rs`（图标服务与 facade 不再自己建客户端）。
- 新增替身测试（图标服务目前没有测试，本卡新建）：图标服务与两个诊断方法各一条，断言替身被以期望参数（含当次的端口）调用；图标抓取与 ASN 查询的错误原样传到调用方，延迟测试的 `Some`/`None` 原样传到调用方（公开返回形状不变）。另加一条：两次调用之间改变 `clash_info` 的端口，替身两次收到的端口不同。
- egress 与 changelog（`C/client/app_update/changelog.rs:318-335`）的现有测试通过。

---

## A5a — 运行时构建编排上移 core

- [x] core 新增 `RuntimeBuildService`（`C/client/` 下），字段 `Arc<ScriptRunnerFactory>`、`Arc<dyn ProfileContentSource + Send + Sync>`、`RuntimePaths`、core-spec 解析器；实现 `RuntimeBuildPort`。逻辑从 `P/enhance/runtime_build.rs:37-107` 原样迁入（含 async 方法内直接读内容、`ProfilesActor::current_closure`、`blocking::join`、`write_product`）。
- [x] `T/setup.rs` 改为构造 `RuntimeBuildService`（闭包来自 scripts 构造函数，内容源仍来自 platform 的 `content_source.rs`）。
- [x] 删除 `P/enhance/runtime_build.rs`；`P/enhance/runtime_build/tests.rs` 迁入 core，内容源换成测试内的内存实现、runner 换成测试替身；若某用例必须真实引擎，core 加 scripts 的 dev-dependency 并在 PR 说明。
- [x] golden（`P/enhance/runtime_build/golden.rs` 与 fixtures）与 `backend/nyanpasu-platform/tests/runtime_builder.rs` 迁入 transform 的 `cfg(test)` 单测模块，transform dev-dependency scripts；夹具经 `cfg(any(test, feature = "test-support"))` 直接可用；内容源用测试内实现读 fixture 目录。

**验证**

- 迁移前后 fixture 文件的 `git hash-object` 一致；单独 `cargo test -p nyanpasu-profile-transform`（不经 workspace 其他包的 feature 请求）下 golden 测试名清单为 7 例且通过。
- core 下严格/宽松用例（原 `P/enhance/runtime_build/tests.rs:72-124`）通过。
- `rg 'nyanpasu_core' backend/nyanpasu-platform/src/enhance` 退出码 1。

## A5b — persistence 与 profile-io（文件部分）

- [x] 新建 `backend/nyanpasu-persistence`：从 `C/corrupt.rs` 移入 `retry_io`、`retry`、`retry_with_delay`（改为 `pub`）、`retry_async`、`is_link`、`preserve`、`Error`、时间戳格式化与独占重命名辅助及其测试；依赖 `backon`、`nyanpasu-utils`（不开 `redb`）、snafu、`time`、tracing、`atomicwrites`、macOS `nix`。`error_io` 与 redb、core-manager 分类留在 core。更新 core 内全部使用者，包括 `retry_with_delay` 的生产调用点 `C/session_state.rs:48`、`C/storage.rs:277`（直接导入，不留转发函数）以及 `migration/{fs,store,child_result}.rs`、`state/profiles/error.rs`。
- [x] 新建 `backend/nyanpasu-profile-io`：移入 `ProfileFsPort`、`ProfileMaterializationPort` 的实现（`P/profiles/{fs_service,materialization}.rs` 对应部分）与 `P/enhance/content_source.rs`；依赖 contracts、config、persistence。订阅抓取留在 platform，tauri 的 `ProfileFilePorts.fetcher` 暂由 platform 提供。
- [x] 同一提交迁完仍在 platform 的测试装配：`P/profiles/test_support.rs:22-30` 改为 profile-io 的实现填 `fs`、`materialization`、platform 的抓取类型填 `fetcher`；`P/profiles/fs_service/integration_tests/{profiles,facade,workflow}.rs` 中把旧类型当作文件或 materialization 端口的地方（如 `profiles.rs:143`）改用 profile-io 的类型；platform 加 profile-io 的 dev-dependency。
- [x] adapter 本地测试随文件进入 profile-io，夹具改用 contracts `test-support`。
- [x] 登记：`FOUNDATION` 加 persistence（附加禁止 `redb`、`nyanpasu-core-manager`，及与 contracts、transform 互不依赖）；`IMPLEMENTATION` 加 profile-io；`STATIC_GATE_PREFIXES` 加两个 `src/`。

**验证**

- 随文件移动的 adapter 测试名清单在迁移前后一致且通过；`evidence_collision_never_overwrites_either_copy` 在 persistence 下通过。
- 单独 `cargo test -p nyanpasu-platform` 与 `cargo test -p clash-nyanpasu --no-run` 通过（platform 测试装配与 tauri 六处测试使用者在本步可编译）。
- 门禁通过；反向检查：给 persistence 临时开 `nyanpasu-utils` 的 `redb` feature，门禁失败。

## A5c — `nyanpasu-net`（整条 HTTP 策略路径，一个提交）

- [x] 新建 `backend/nyanpasu-net`：订阅抓取拆为 `SubscriptionHttpFetcher` 实现 `SubscriptionFetcher`，字段为 self proxy 端口源、超时、元数据、`SystemProxySource`、四个设备头值；连同 `P/profiles/subscription.rs` 与 `C/utils/key_value.rs` 移入。代理优先级、默认 UA、重试原样保留。
- [x] 同一提交移入 `P/changelog_http.rs`、`C/client/direct_egress.rs:35-105` 的实现、`C/net_adapters.rs` 的图标与网络诊断实现、客户端策略 `get_reqwest_client` 与 `NyanpasuReqwestProxyExt`（系统代理经 `SystemProxySource`）。
- [x] tauri：从 `nyanpasu_core::utils::hwid` 取设备信息，按原逻辑（含 `sanitize_for_header`）准备四个头值；`ProfileFilePorts.fetcher` 改由 net 提供；`T/setup.rs` 构造 net 的其余实现并注入；`T/adapters/app_update.rs:88-90` 改用 net 的策略。
- [x] core 删除 `C/net_adapters.rs` 与 `utils/{candy,config,net,key_value}.rs` 中已无使用者的部分；`SystemProxySource` 的 core 实现保留到 C2；updater 的独立客户端（`C/updater/instance.rs:78-89`）不动。
- [x] 订阅相关 adapter 测试随文件进入 net；platform 的测试装配与集成测试同一提交改用 net 的抓取类型（platform 加 net 的 dev-dependency），tauri 的六处测试使用者随之可编译。
- [x] 登记：`IMPLEMENTATION` 加 net；`STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-net/src/`。

**验证**

- `rg 'NyanpasuReqwestProxyExt|get_reqwest_client' backend/nyanpasu-core backend/tauri backend/nyanpasu-platform` 退出码 1。
- 新增确定性测试：给定夹具设备信息，订阅请求的 `x-hwid`、`x-device-os`、`x-ver-os`、`x-device-model` 精确等于期望值（含净化后的值）；`fetch_sends_default_user_agent_and_hwid` 通过；订阅 adapter 测试名清单迁移前后一致。
- 单独 `cargo test -p nyanpasu-platform` 与 `cargo test -p clash-nyanpasu --no-run` 通过。
- 门禁通过；反向检查：给 profile-io 临时加 net 依赖，门禁失败；A4b 的替身测试仍通过。

## A5d — 迁出 platform 的测试并退役 platform

- [x] 应用集成测试 `P/profiles/fs_service/integration_tests/{profiles,facade,workflow}.rs` 移入 core 测试，core dev-dependency profile-io 与 net，真实适配器在测试本地组装；`P/profiles/test_support.rs` 的组装职责移到 tauri 测试本地支撑，`T/unified_rpc.rs:486,875,964,1087,1151`、`T/adapters/hotkey/tests.rs:27` 改用它。
- [x] 确认 platform 已无内容，删除 crate 目录、workspace 项、`backend/tauri/Cargo.toml:42,268`。
- [x] 门禁：删除 platform 及"platform → core 允许"用例。
- [x] 文档：`docs/development/backend-packages.md`、`docs/development/architecture.md`、`AGENTS.md` §5、路线图的 Delivered baseline、Known gaps、Phase A 状态，按实际结果更新。
- [x] 按 design §6 复测编译时间，写进 PR 描述（#6171）；O-3 按数据建议 b，待维护者确认（design §8.2）。

**验证**

- 三组集成测试的测试名清单迁移前后一致且通过。
- `cargo metadata --no-deps --format-version 1` 不含 `nyanpasu-platform`；`rg 'nyanpasu[-_]platform' backend scripts docs/development AGENTS.md` 退出码 1。
- 全量：`cargo clippy --workspace --all-targets --all-features`、`cargo test --workspace`、`pnpm run lint:backend-boundaries`、`pnpm run lint:architecture-ledger`、`pnpm run test:scripts`、三个生成文件无 diff。

---

## 2. Phase B 的 PR 划分

本节是 design §5.1 的细化，行号记录于 `5aa056c1f`（Phase A 结束），按符号定位。细化中的设计偏差见 design §8.1 V-10–V-15。全局约束 §0 照旧适用；新 crate 的固定清单同 design §4.6，另加两项：

- musl 手动测试脚本（`scripts/src/testing/test-backend-musl.ts:68,77-84`）的 `-p` 列表与 `expectedTargets` 同时加入新 crate（以及新增的 `tests/*.rs` 目标）。
- 旧路径检查用 `rg` 的显式文件参数或 `--glob`，不用 shell 花括号展开；需要排除测试代码时写明排除方式。依赖方向以门禁（`cargo tree -e normal,build`）为准，`rg` 只查导入路径，不查注释里的词。

| PR    | 卡                                | 依赖     | 说明                                                         |
| ----- | --------------------------------- | -------- | ------------------------------------------------------------ |
| PR-B1 | B1a                               | A        | 一个提交                                                     |
| PR-B2 | B1b-1 → B1b-2                     | B1a      | 先在 core 内切断外向边，再整体移入 `nyanpasu-migration`      |
| PR-B3 | B2a、B2b                          | B1b      | 两个独立提交                                                 |
| PR-B4 | B3a → B3b → B3c → B3d → B3e → B3f | B2a、B2b | 先切断 traffic、geo 的 core 依赖，再逐个 owner 移入新 crate  |
| PR-B5 | B4a → B4b                         | B3a      | 用 B3a 的 owner 支撑 crate；B 全部完成后复测，写进本 PR 描述 |

新 crate 共四个：`nyanpasu-migration`、`nyanpasu-owner-support`、`nyanpasu-observers`、`nyanpasu-updater`，都进 `CORE_FAMILY`（名字是 O-4 的工作名，在创建它的 PR 中定）。具体适配器（redb 日志库、traffic 文件库与 legacy 导入、geo 文件监视与缓存、Clash 原始帧归一化、core 模式跟随）留在 `nyanpasu-core`，由 core 实现下层 owner 的端口（V-12）。

文档同步：创建 crate 的卡同时更新 `docs/development/backend-packages.md` 的包表与 `docs/development/architecture.md` 的布局；B4b 更新路线图（`docs/design/application-platform-extraction.md`）的 Delivered baseline 与 Phase B 状态。

---

## B1a — persistence 收格式与通用写入

- [x] `C/format.rs` 整体移为 `backend/nyanpasu-persistence/src/format.rs`（`Format`、`YamlFormat`、`STAMP_KEY`、`DocumentStamp`、`StampError`、`Inspected`、`inspect`、`stamp`、`StampedDocument`、`StampedYamlFormat` 与 4 个测试），内容不变；删除 `C/lib.rs` 的声明。
- [x] `C/migration/fs.rs` 的写入错误 `Error`（五个变体原样）、`write_document`、`atomic_write`（含重试分类与最后的 `context`，改为 `pub`）移入 persistence 新模块 `write`；读取侧（`try_exists`、`read_yaml_if_exists*`、`DocumentFile`、`read_document*`）与其 4 个测试留在 migration。`YamlFormat` 前缀后一个换行、`write_document` 前缀后两个换行的差异保持不变。
- [x] persistence 加 `serde`、`serde_yaml_ng`、`anyhow`、`thiserror`（版本与 feature 同 core 现有声明）；不加 config、contracts。
- [x] `C/session_state.rs:25,70` 改用 `nyanpasu_persistence::write`。其余调用方直接改导入，不留转发：core 的 `state/manager/{persistent_builder,persistent_state,weak_persistent_state}.rs`、`state/tests/support.rs`、`migration/{fs,mod,store,child_result,runner}.rs`、`migration/modules/{app_config,application,clash_config,profiles,storage,typed_config}.rs`、`client/{mod.rs,profiles.rs,test_support.rs}`、`client/application_workflow/tests/mutations.rs`、`profile_integration_tests/profiles.rs`；tauri 的 `utils/collect.rs`、`utils/init/mod.rs`、`unified_rpc.rs` 中对 `nyanpasu_core::format` 的引用。`Failure::from_error` 对写入错误的 downcast（`child_result.rs:159`）改为新类型；tauri 不直接 downcast 它，启动失败分类经 `Failure::from_error`，由下面的测试覆盖。

**验证**

- `rg 'crate::format|super::format' backend/nyanpasu-core/src`、`rg 'nyanpasu_core::format' backend/tauri/src`、`rg 'migration::fs::(atomic_write|write_document|Error)' backend/nyanpasu-core/src backend/tauri/src`、`rg 'migration::' backend/nyanpasu-core/src/session_state.rs` 均退出码 1。
- 测试名清单：persistence = 移动前 + format 的 4 例；core = 移动前 − 这 4 例；`session_read_retries_transient_io_at_its_production_read_call_site`、`windows_sharing_and_lock_violations_retry_without_quarantining`、`evidence_collision_never_overwrites_either_copy`、`atomic_write_errors_keep_hidden_io_classification` 通过。
- 门禁通过（persistence 仍不依赖 redb、core-manager、contracts、transform）；`cargo test -p nyanpasu-persistence`、`-p nyanpasu-core`、`-p clash-nyanpasu`；`T/startup_failure_tests.rs` 与 `T/utils/init/tests.rs` 通过。

---

## B1b-1 — 在 core 内切断 backup 与 migration 的外向边

本卡不是纯移动：引入两个存储端口与一个参数，行为不变。

- [x] 改动前，在当前提交上为将被替换 source 的错误变体（`BackupError::Storage`、`C/migration/mod.rs` 中以 `StorageOperationError` 为 source 的变体、`C/migration/modules/storage.rs` 的打开/读/写/删除错误）生成期望值测试：`Display`、逐级 `source()` 文本，若该错误经 serde 序列化则加 JSON；用真实 redb 错误构造，确认通过。这些测试从一开始就放在 core 的存储模块（`C/storage.rs` 的测试），B1b-2 只改其导入路径，不进 migration。
- [x] backup：在 `C/backup.rs` 定义消费者自有端口：

  ```rust
  pub trait StorageSnapshot: Send + Sync {
      fn export_to(&self, destination: &Path) -> Result<(), BoxedCause>;
  }
  ```

  `StorageSource::Live` 改持 `&dyn StorageSnapshot`；`BackupError::Storage` 的 source 改为 `BoxedCause`，`Display` 不变。`C/storage.rs` 为 `Storage` 实现它，委托 `Storage::export_to`（保持 `:113` 的单个读事务），在产生处装箱 `StorageOperationError`。`C/client/mod.rs:759-767` 照旧克隆 `self.inner.storage` 并传 `&storage`（字段类型在 B2a 换）。修复 `C/backup.rs:4` 指向 `crate::client::NyanpasuClient` 的 rustdoc 链接（B1b-2 后不可解析）。

- [x] migration：在 `C/migration/modules/storage.rs` 定义消费者自有的打开器与句柄：

  ```rust
  pub trait HotkeyStoreOpener: Send + Sync + std::fmt::Debug {
      fn open(&self, path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause>;
  }
  pub trait HotkeyStore {
      fn read(&self) -> Result<Option<Vec<String>>, BoxedCause>;
      fn write(&self, hotkeys: &[String]) -> Result<(), BoxedCause>;
      fn remove(&self) -> Result<(), BoxedCause>;
  }
  ```

  打开器无状态，由 core 的 `Storage` 侧实现（`C/storage.rs`，内部 `Storage::try_new` 与现有的键名、序列化）。每次操作打开、用完即关的时机不变：写句柄在重写旧 YAML 前结束（`:154-165`）、读句柄在 `read_kv_hotkeys` 返回时结束（`:270-280`）、先提交类型化 YAML 再重新打开删除（`:252-262`）；"库是否存在"的判断与目录创建留在原调用点；打开、读、写、删除各自的错误上下文保留，source 改为 `BoxedCause`。

- [x] 打开器经 `Ctx` 到达模块：`Ctx` 加字段 `Arc<dyn HotkeyStoreOpener>`（保留 `Debug`、`Clone` 派生）；`Ctx::from_paths`、测试用 `Ctx::new`、`Runner::with_target`、`Runner::with_paths` 增加该参数。本卡迁完全部构造方：`T/setup.rs:244`、`T/cmds/migrate.rs:101`、`C/client/startup_recovery_tests.rs:91,367`、`C/client/test_support/startup_recovery.rs:25`、`C/migration/modules/profiles.rs:1302`，以及 `runner.rs`、`modules/{storage,typed_config,profiles}.rs` 测试里的夹具构造；它们在本卡都传 core 的真实打开器。
- [x] `Failure::from_error`（`C/migration/child_result.rs:77`）增加参数 `io_failure: Option<IoFailure>`，删除 `:204` 对 `crate::corrupt::error_io` 的调用；三处生产调用方 `T/cmds/migrate.rs:106,163`、`T/startup_failure.rs:57` 对传给 `from_error` 的同一个错误调用 `nyanpasu_core::corrupt::error_io` 并传入，测试（`T/startup_failure_tests.rs:198,215`）同改。`error_io` 本身不改（非 IO 的 redb 失败提前返回、递归遍历 source）；`Failure` 的 JSON 字段不变。
- [x] `C/utils/yaml.rs` 移为 `C/migration/yaml.rs`（唯一使用者 `modules/typed_config.rs:10,37,434`）。
- [x] backup 与 migration 的测试不再调用 `crate::utils::test_support::test_paths`（`C/backup.rs:372,504`、`C/migration/mod.rs:99`、`runner.rs:736`、`modules/profiles.rs:1301`），改为直接调用 `PathResolver::with_base_dirs`。

**验证**

- 第一步的期望值测试原样通过（替换后 `Display` 与 source 链不变）；反向检查：把一处装箱改成 `BoxedCause::from(e.to_string())`，期望值测试变红。
- `C/migration/**` 与 `C/backup.rs` 中 `#[cfg(test)]` 模块以外的代码，`rg 'crate::(storage|corrupt|utils)'` 无匹配；测试代码中剩余的匹配在 PR 描述列出，由 B1b-2 按下表处理。
- backup 11 例、migration 148 例的测试名清单不变且通过（core 另加本卡新增的期望值测试）；`T/startup_failure_tests.rs`、`T/utils/init/tests.rs:239,322` 通过；三个生成文件无 diff。

## B1b-2 — `nyanpasu-migration`

- [x] 新建 `backend/nyanpasu-migration`：`C/migration/**`（含 `fixtures/`、`legacy_schema/`、`modules/`、`yaml.rs`）、`C/backup.rs`、`C/session_state.rs` 移入。crate 根是原 `migration/mod.rs` 的内容，`backup`、`session_state` 为其子模块（V-10）。
- [x] 依赖按文件实际使用声明（config 关默认 feature 并列出所需，contracts、persistence、`nyanpasu-paths`、`nyanpasu-utils` 不开 `redb`，及这些文件实际用到的第三方库）；不依赖 redb、`nyanpasu-core-manager`、core。
- [x] 构造真实 `Storage` 的用例按下表放置，名字不变：

  | 用例                                                                                                                                                                                                                                                                                     | 现位置                                   | 去向                                   |
  | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | -------------------------------------- |
  | `a_live_storage_is_exported_as_a_consistent_copy`                                                                                                                                                                                                                                        | `C/backup.rs:595`                        | core（真实 redb 快照与重新打开导出库） |
  | `real_1_6_1_fixture_migrates_to_2_0_shape`、`full_chain_keeps_the_key_value_hotkeys`                                                                                                                                                                                                     | `C/migration/runner.rs:902,994`          | core（真实存储全链路）                 |
  | `migrates_kv_hotkeys_into_typed_config`、`is_idempotent_when_kv_key_absent`、`kv_value_replaces_typed_hotkeys`、`explicitly_empty_kv_list_clears_typed_hotkeys`、`refuses_to_drop_hotkeys_before_the_typed_config_exists`、`empty_legacy_hotkeys_list_satisfies_its_check_after_the_run` | `C/migration/modules/storage.rs:364-456` | migration，改用替身（见下）            |

  替身打开器：打开新库时在该路径创建一个空的标记文件（生产代码的"库是否存在"检查留在 `:283`，照旧看文件系统），按路径在多个句柄之间保留 hotkeys，区分"键不存在"与"显式空列表"。其余用 `LegacyCase` 等 fixture 的用例留在 migration，用同一个替身。移到 core 的三例放进 core 的 migration 集成测试模块，用公开的 `Runner::with_target` 与真实打开器组装，断言时重新读取 `MigrationStore`，不暴露 `Runner` 私有项（`runner.rs:926` 的 `with_context`、`:948` 的 `runner.store`）；fixture 只在 migration 保留一份，由 migration 的 `test-support` feature 导出所需内容（或改相对路径），迁移前后 `git hash-object` 一致。core 以 dev-dependency 开启 migration 的 `test-support`，并在 core 的 `test-support` 中转发（§0.5）。

- [x] 调用方迁完：core 的 `client/{mod.rs,application.rs,clash_config.rs,profiles.rs,session_state.rs}`、`state/{application.rs,clash_config.rs,profiles/actor.rs}` 与测试（`client/{mod.rs,test_support.rs,startup_recovery_tests.rs,test_support/startup_recovery.rs}`、`application_workflow/tests/{mutations,startup}.rs`、`profile_integration_tests/profiles.rs`）；tauri 的 `setup.rs`、`cmds/migrate.rs`、`utils/init/mod.rs`、`utils/collect.rs`、`startup_failure.rs`、`ipc.rs` 与测试（`unified_rpc.rs`、`startup_failure_tests.rs`、`utils/init/tests.rs`）。core 不再有 `migration`、`backup`、`session_state` 模块。
- [x] 登记：workspace；`CORE_FAMILY` 加 migration，附加禁止 `redb`、`nyanpasu-core-manager`，正反策略测试；`STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-migration/src/`；`S/policy.ts:52-53`（排除）、`:115`（模块允许名单）、`:171`（registry）、`:246`（测试 static）改到新路径并同步改其测试；musl 脚本；`backend-packages.md`、`architecture.md`。

**验证**

- `rg 'crate::(migration|backup|session_state)|super::migration' backend/nyanpasu-core/src`、`rg 'nyanpasu_core::(migration|backup|session_state)' backend` 退出码 1；`rg 'use (nyanpasu_core|redb|nyanpasu_core_manager)|(nyanpasu_core|redb|nyanpasu_core_manager)::' backend/nyanpasu-migration/src` 退出码 1（注释中描述 redb 的文字不算）。
- `cargo tree -p nyanpasu-migration -e normal,build --all-features` 不含 redb；反向检查：给 migration 临时开 `nyanpasu-utils` 的 `redb` feature，门禁失败。
- 测试名清单：移动前 core 中 backup、migration、session_state 的用例，等于移动后 migration 的清单加上移到 core 的三例（只允许前缀变化）；B1b-1 新增的期望值测试仍在 core；全部通过。
- 单独 `cargo test -p nyanpasu-migration`、`-p nyanpasu-core`、`-p clash-nyanpasu` 通过；三个生成文件无 diff。

---

## B2a — facade 不再持有具体存储

本卡不是纯移动：traffic 存储换成非泛型的 owner 级端口。

- [x] `ClientSetupArgs.storage: Storage`（`C/client/mod.rs:111`）与 `NyanpasuClientInner`（`:232`）的对应字段改为 `Arc<dyn nyanpasu_migration::backup::StorageSnapshot>`，只用于在线备份；`:767` 改为 `StorageSource::Live(storage.as_ref())`。tauri `T/setup.rs:439` 传入 `Arc::new(storage.clone())`；KV 命令与变更订阅继续用 shell 持有的 `Storage`（不变）。
- [x] traffic 模块新增 owner 级端口（消费者自有、非泛型、对象安全）：

  ```rust
  #[async_trait::async_trait]
  pub trait TrafficOwnerFactory: Send + Sync {
      async fn spawn(
          self: Arc<Self>,
          args: TrafficStartArgs,
          shutdown: CancellationToken,
          tasks: &TaskTracker,
      ) -> Result<TrafficClient, SpawnError>;
  }
  ```

  `TrafficStartArgs` 是 `TrafficArgs` 去掉 `storage` 后的字段；`SpawnError` 是现有的 `traffic::client::Error`（公开导出，消息与 source 链不变）。`TrafficFileStorage` 实现它：把 `self` 放进 `TrafficArgs<RedbStorage>` 后调用现有的 `TrafficClient::spawn`。

- [x] `ClientSetupArgs.traffic_store`（`:129-130`）与 `NyanpasuClientInner::new` 的同名参数（`:409-410`）改为 `Option<Arc<dyn TrafficOwnerFactory>>`。调用点保持在 `:604-633`：`None` 不创建；`Err` 打印原告警并得到 `None`；都不让启动失败。tauri `open_traffic_store`（`T/setup.rs:605`）改返回新类型，打开数据库的时机不提前。
- [x] 测试装配（`C/client/test_support.rs`、`test_support/facade.rs`、`startup_recovery_tests.rs` 等）同改。

**验证**

- `ClientSetupArgs` 定义（`C/client/mod.rs:103-136`）与 `NyanpasuClientInner::new` 的参数中无具体存储类型（`nyanpasu_core::storage::Storage`、`RedbStorage`、`redb::`、`nyanpasu_tsdb::storage`）；字段引用的 core 自有类型（`ClientHostAdapters`、`LoggingSetup` 等）逐个检查，结果写进 PR 描述。
- `a_live_storage_is_exported_as_a_consistent_copy`、traffic 启动与关闭测试、`cargo test -p nyanpasu-core`、`-p clash-nyanpasu` 通过。

## B2b — traffic 的 legacy 导入器放到 feature 后

- [x] `backend/nyanpasu-traffic/Cargo.toml`：`redb` 改为可选，新增 `legacy-redb = ["dep:redb"]`，默认 feature 不含它；`store/legacy.rs`、导入与放弃 API、`TrafficDb` 中只为导入存在的字段与路径以该 feature 门控。开 feature 时行为逐字节不变（进度键、编码、已提交前缀、完成标记，`store/mod.rs:65-79` 的恢复）；关 feature 时 store 照常打开与记录，不读写进度键。
- [x] 集成测试目标 `tests/legacy_v3.rs`（含 `v3_reports_match_the_old_implementation_byte_for_byte`、忽略的 `export_v3_fixture`）在 `Cargo.toml` 用 `[[test]] required-features = ["legacy-redb"]` 门控；`tests/legacy_v3/{fixture.rs,oracle.rs,expected.json,README.md}` 不变，README 中重新生成的命令加上该 feature。
- [x] core 对 `nyanpasu-traffic` 的依赖开启 `legacy-redb`（`C/traffic/storage.rs` 的启动导入在 core）；导入、失败后丢弃重开、持久放弃、旧句柄关闭后才删除、暴露会话前重开的顺序（`C/traffic/storage.rs:49-75`）不变。

**验证**

- `cargo tree -p nyanpasu-traffic --no-default-features -e normal,build` 不含 redb；反向检查：把 redb 改回无条件依赖，该检查失败。
- `cargo test -p nyanpasu-traffic`（默认）与 `--features legacy-redb` 都通过；legacy 的 5 例单测与 `legacy_v3` 目标只在开 feature 时出现且通过；core 的 `TS` 6 例（`C/traffic/storage.rs:131-316`）通过。

---

## B3a — `nyanpasu-owner-support`

- [x] 新建 `backend/nyanpasu-owner-support`：`C/owner_shutdown.rs`（`drain_on_shutdown`、`track_until_shutdown`）与 `C/utils/blocking.rs`（`join`）原样移入（V-13），删除两处旧模块声明；依赖只有 ractor、tokio、tokio-util。
- [x] `owner_shutdown` 的 18 个使用文件：core 的 `actor_v2/service_actor.rs`、`clash/ws.rs`、`geo/client.rs`、`logs/actor.rs`、`traffic/client.rs`、`updater/mod.rs`、`proxies.rs`、`client/{app_lifecycle,app_update/mod,application,application_workflow/mod,clash_config,effects/actor,hotkey/mod,profiles,session_state,system_proxy/mod}.rs`，tauri `setup.rs`。
- [x] `utils::blocking` 的使用文件：core 的 `geo/actor.rs`、`traffic/actor.rs`、`service/control.rs`、`client/{jobs,mod,profiles,runtime,runtime_build,runtime_inspection,session_state}.rs`、`client/core_lifecycle/adapters.rs`、`client/system_proxy/actor.rs`、`client/test_support/runtime_build.rs`，tauri 的 `utils/collect.rs`、`adapters/system_proxy.rs`。
- [x] 登记：workspace；`CORE_FAMILY` 加它，附加禁止 `CORE_FAMILY` 的其他成员（它是 core 族最底层），正反策略测试；`STATIC_GATE_PREFIXES`；musl 脚本；`backend-packages.md`。

**验证**

- `rg 'crate::owner_shutdown|crate::utils::blocking|nyanpasu_core::(owner_shutdown|utils::blocking)' backend/nyanpasu-core/src backend/tauri/src` 退出码 1。
- `drain_and_wait(None)`、取消优先于生产者完成、`join` 对取消与 panic 的处理（`C/utils/blocking.rs:6-12`）原样；`app_lifecycle` 的关闭测试与 `C/updater/tests.rs:281,346` 通过。

## B3b — traffic 的 Clash 原始帧归一化移到 owner 之上

本卡不是纯移动：新增一个归一化端口与一个非泛型的帧输入，泵的行为不变。

- [x] `C/traffic/source.rs` 全部与 `C/traffic/geo.rs` 中解释 Clash 原始数据的部分（`clash_api` 类型、`ClashConnectionsFrame` → `Frame` 的转换、核心给出的地区代码解释）移到 owner 之上的 core 模块（`C/client/traffic.rs` 旁，按内容命名）。`LocalSourceIps` 与本地源端口留在 owner。
- [x] `C/traffic/ports.rs` 定义消费者自有的归一化端口，参数照搬泵现在传给转换函数的值：

  ```rust
  pub trait FrameNormalizer: Send + Sync + 'static {
      type Raw: Send + Sync + 'static;
      fn normalize(&self, raw: &Self::Raw, wall_ms: i64, mono: Duration,
                   index: Option<&IpIndex>, local: LocalSourceIps) -> Frame;
  }
  ```

- [x] `TrafficArgs.frames` 与 B2a 的 `TrafficStartArgs.frames` 改为非泛型的 `TrafficFeed`：由泛型构造函数 `TrafficFeed::new<N: FrameNormalizer>(watch::Receiver<Option<Arc<N::Raw>>>, Arc<N>)` 生成，内部保存一个一次性的泵启动闭包；owner 在原 `pre_start` 位置用它启动现有的泵（泛型于 `N`），不新增任务、通道或缓冲。泵的语义原样（`C/traffic/actor.rs:319-342`）：等 `changed`、经 `borrow_and_update` 克隆、采样当前已发布索引、再取时钟/单调时间/本地地址、同步归一化、等待真实回复；`:220-234` 的中止、等待与 panic 传播在最后 flush 前不变。core 用 Clash 的归一化实现构造 `TrafficFeed`（`C/client/mod.rs:620` 附近）。
- [x] 归一化的测试（`TG` 13 例、`frame_from_snapshot_extracts_dimensions`、`frame_from_snapshot_maps_the_inbound_and_the_regions`）随实现移动；三个 `pump_*` 用例改为经 core 的归一化实现组装，暂留 traffic 测试文件，B3e 时留在 core。

**验证**

- `rg 'clash_api|crate::clash|ClashConnectionsFrame' backend/nyanpasu-core/src/traffic/actor.rs backend/nyanpasu-core/src/traffic/client.rs backend/nyanpasu-core/src/traffic/ports.rs backend/nyanpasu-core/src/traffic/mod.rs` 退出码 1。
- traffic 的测试名清单不变（只允许被移动用例的前缀变化）且通过；`restart_with_the_same_instance_does_not_recount`、`recovery_between_core_changing_close_groups_preserves_instance_and_events`、`pump_delivers_frames_and_lost_feeds`、`pump_locates_regions_with_the_published_index`、`pump_maps_local_regions_without_changing_source_identity_or_usage` 通过。

## B3c — geo 不再持有 `CoreClient`，监视错误不泄漏 `notify`

本卡不是纯移动：新增一个模式来源端口与错误装箱。

- [x] `C/geo/ports.rs` 定义消费者自有端口 `pub trait ModeSource: Send + Sync + 'static { fn start(&self, reporter: ModeReporter) -> JoinHandle<()>; }`；`ModeReporter`（`Send`，可克隆）的 `report(&self, GeodataMode) -> bool` 包装对 owner 的私有 `cast(Message::Mode(_))`，投递失败返回 `false`。owner 的启动参数（`C/geo/actor.rs:41` 进入 `Actor::Arguments` 的结构）以 `Arc<dyn ModeSource>` 替换 `CoreClient`。
- [x] `follow_core`（`C/geo/actor.rs:135-160`）原样移到 owner 之上的 core 适配器并实现 `ModeSource`：一秒重试、`geodata_mode` 缺省按 MMDB、每个租约成功报告一次、`api.cancelled().await` 后重新获取、`report` 返回 `false` 时结束。owner 在 watcher 建好之后、原 `:72` 位置调用 `start`，句柄照旧存入 `State`，`post_stop` 的中止、等待与 panic 传播（`:89`）不变；owner 参数不再含 `CoreClient`。
- [x] `GeoIndexError` 中以 `notify` 错误为 source 的变体（`C/geo/ports.rs:48`）改为 `BoxedCause`，`Display` 不变；`C/geo/adapters.rs:159,162` 两个构造点装箱原始 notify 错误（不是文本）。先为该变体写期望值测试（`Display` 与 source 链）再改。
- [x] `C/geo/tests.rs` 的 3 例改用 `ModeSource` 替身，不再用 `crate::actor_v2::test_support`（`:26,34`）。新增 core 适配器的确定性测试，用 `actor_v2::test_support` 的端点注入（`:254`）与绑定发送端（`:266`）：未取得租约时按间隔重试、租约撤销后重新获取并再报告一次、`report` 返回 `false` 后跟随任务结束。测试里的 `ModeReporter` 由 owner 提供的 `test-support` 构造函数生成（B3f 后来自 observers）。

**验证**

- `rg 'CoreClient|actor_v2|notify_debouncer_full|notify::|clash_api' backend/nyanpasu-core/src/geo/actor.rs backend/nyanpasu-core/src/geo/client.rs backend/nyanpasu-core/src/geo/ports.rs backend/nyanpasu-core/src/geo/mod.rs` 退出码 1（`OnChange::notify` 方法名不算）。
- geo 的测试名清单 = 移动前 + 新增的适配器测试，全部通过，含 `a_written_database_waits_for_the_core_s_mode`、`a_failed_reload_keeps_the_index_and_its_key`；期望值测试原样通过。

## B3d — `nyanpasu-observers` 与 logs owner

- [x] 新建 `backend/nyanpasu-observers`，模块 `logs`、`traffic`、`geo`（V-12）；本卡只移入 logs owner：`C/logs/{actor.rs,model.rs,ports.rs}` 与 owner 客户端。redb 日志库与编解码（`redb.rs`、`codec.rs`、压缩字典资源）、`test_support.rs`、`tests.rs` 留在 core。
- [x] 可见性：`RedbCoreLogStore` 用到的 `PreparedCoreLog` 字段 `bytes`、`level`（`C/logs/model.rs:73`）改为公开的只读访问器，`level_code`、`truncate_preview`（`C/logs/redb.rs:212,364,394` 的使用）与所需常量公开导出，不公开可变字段。`CoreLogsClient::flush`（现为 `pub(super)`）改为 `pub`，它与对应的消息变体、处理分支（`C/logs/actor.rs:23,314,423`）统一门控为 `cfg(any(test, feature = "test-support"))`；core 以 dev-dependency 开启 observers 的 `test-support` 并在 core 的 `test-support` 中转发；actor 消息保持私有。observers 自身的测试不依赖 core 打开该 feature 即可单独运行。
- [x] `C/logs/test_support.rs:42` 的 `impl CoreLogsClient`（跨 crate 不能写固有 impl）改为 core 内的自由函数 `test_client()`，更新 `C/logs/tests.rs:607`、`C/clash/ws.rs` 的 9 处、`C/client/effects/tests.rs:910`；真实 redb 的 `TestStore` 留在 core。
- [x] 调用方迁完：core 的 `clash/ws.rs`、`client/{mod,logs,clash_streams}.rs`、`client/effects/executor.rs:35` 与 effects 测试，tauri 的 `setup.rs`、`ipc.rs`、`unified_rpc.rs`、`adapters/events.rs:11`、`utils/collect.rs:6`。`model.rs` 的 serde/specta 属性原样，三个生成文件无 diff。
- [x] 依赖：owner-support、config（关默认 feature）及实际用到的库；登记 workspace、`CORE_FAMILY`（附加禁止 `redb`、`clash-api`、`nyanpasu-core-manager`、`notify`、`notify-debouncer-full`，正反策略测试）、`STATIC_GATE_PREFIXES`、musl 脚本、`backend-packages.md`、`architecture.md`。redb 只能经 dev-dependency 进入 observers，不经任何普通 feature（含 `test-support`）转发。

**验证**

- `rg 'crate::logs::(actor|model|ports)|nyanpasu_core::logs::(CoreLogsClient|CoreLogRecord)' backend` 退出码 1；`rg 'use (redb|clash_api|nyanpasu_core)|(redb|clash_api|nyanpasu_core)::' backend/nyanpasu-observers/src` 退出码 1。
- logs 的 23 个测试声明（含忽略与按平台互斥的用例）仍在 core，清单不变且通过；反向检查：给 observers 临时加 redb 普通依赖，门禁失败。

## B3e — traffic owner 移入 observers

- [x] `C/traffic/{actor.rs,client.rs,ports.rs}`（含 B2a 的 `TrafficOwnerFactory`、B3b 的 `FrameNormalizer`、`TrafficFeed`）移入 `nyanpasu-observers/src/traffic/`；`TrafficFileStorage` 与启动导入/放弃策略（`storage.rs`）、B3b 的归一化留在 core，core 的 `TrafficFileStorage` 实现 `TrafficOwnerFactory`。
- [x] `C/traffic/tests.rs` 拆分：`startup_imports_v3_and_continues_the_restored_session`（`:330`，用 core 的 `TrafficFileStorage`）与三个 `pump_*`（`:1744,1771,1798`，用 core 的归一化）留在 core；其余 28 例移入 observers，共享夹具改用中立的空闲帧输入或替身归一化。observers 需要真实 tsdb 时只经 dev-dependency 开 redb 后端；traffic 的测试钩子（`C/traffic/actor.rs:27` 的消息变体与 `:132-142` 的处理、`C/traffic/client.rs:122` 的客户端方法）按 B3d 的方式处理：客户端方法 `pub`，三处统一门控为 `cfg(any(test, feature = "test-support"))`，消息保持私有；observers 不依赖 core。
- [x] observers 依赖 `nyanpasu-traffic`、`nyanpasu-tsdb` 时不开 `legacy-redb`、redb。
- [x] 调用方迁完：core 的 `client/{mod,traffic}.rs`、测试装配，tauri 的 `setup.rs`、`ipc.rs` 等。

**验证**

- `rg 'use (redb|clash_api|nyanpasu_core)|(redb|clash_api|nyanpasu_core)::' backend/nyanpasu-observers/src --glob '!**/tests.rs'` 退出码 1；`cargo tree -p nyanpasu-observers -e normal,build --all-features` 不含 redb。
- traffic 的测试名清单（owner 与 core 两侧合计）不变且通过；B3b 列出的五例通过；三个生成文件无 diff。

## B3f — geo owner 移入 observers

- [x] `C/geo/{actor.rs,client.rs,ports.rs}` 与 3 个 owner 测试移入 `nyanpasu-observers/src/geo/`；文件监视与缓存（`adapters.rs`）及其 7 个测试、B3c 的模式跟随留在 core。
- [x] `C/geo/fixtures.rs`（`geoip_dat` 与其私有编码辅助）整体移到 observers，门控为 `cfg(any(test, feature = "test-support"))`，owner 测试、core 的适配器测试与 traffic 归一化测试（`C/traffic/geo.rs:421` 移动后的位置、`C/traffic/tests.rs:1773,1800`）都直接用它，只保留一份。
- [x] `GeoIndexError` 的 snafu 选择器（现为 `visibility(pub(super))`，`C/geo/ports.rs:35`）改为 `pub`，供 core 适配器使用（`IndexDatabaseSnafu`、`ListHomeSnafu`、`ReadDatabaseSnafu`、`WatchHomeSnafu`）；公开导出 `GeoIndexError`、`GeodataMode`、`IndexKey`、`Loaded`、`OnChange` 与 B3c 的 `ModeSource`、`ModeReporter`。
- [x] 调用方迁完：core 的 `client/mod.rs`、traffic 归一化、测试，tauri 的 `setup.rs`。

**验证**

- `rg 'use (notify|notify_debouncer_full|clash_api|nyanpasu_core)|(notify_debouncer_full|clash_api|nyanpasu_core)::|CoreClient' backend/nyanpasu-observers/src` 退出码 1；`rg 'crate::geo::(actor|client|ports)' backend/nyanpasu-core/src` 退出码 1。
- geo 的测试名清单（两侧合计）不变且通过；门禁通过。

---

## B4a — updater 的 core 外向边下移

- [x] `PreparedCoreBinary`、`PreparedCoreCompanion`、`BinaryInstallProgress`（`C/client/core_lifecycle/ports.rs:17,30,43`）移到 `C/updater/` 下新文件（按内容命名）；core_lifecycle 的运行时准备端口（`:84` 起）不动。更新使用者：`C/updater/{ports,instance,tests}.rs`、`C/client/application_workflow/mod.rs:33,838`、`C/client/core_lifecycle/{mod,workflow,adapters}.rs`、`C/client/mod.rs:1489,1494`、`C/client/test_support/workflow.rs`、`application_workflow/tests/{mod,startup,connection_policy}.rs`。
- [x] `parse_version` 与 `CoreVersionError`（`C/client/core_version.rs:6-50`，含生成的 `RunCoreVersionSnafu`、`CoreVersionExitSnafu`、`CoreVersionNotReportedSnafu`）移到 `nyanpasu-contracts` 新模块 `core_version`（V-15）：`#[snafu(visibility(pub))]`、`#[serde(skip)]`、`tag = "kind"` 的枚举形状、`#[specta(type = String)]` 原样；`parse_version` 返回原匹配 token（保留 `v`、`alpha-` 或 premium 前缀）。contracts 加 `semver = "1.0"`。`CoreVersionReader` 端口留在 `C/client/core_version.rs`。更新 `C/updater/instance.rs:670`、`C/client/{mod.rs,runtime_error.rs}`、`T/utils/core_version.rs:6,13,44` 等使用者。
- [x] `C/utils/candy.rs`（唯一使用者 `C/updater/instance.rs:27,111`）移到 `C/updater/` 下（按内容命名），含 `test_mirror_speed_test`（联网用例，不是确定性回归测试）。

**验证**

- `rg 'crate::client|crate::utils::candy' backend/nyanpasu-core/src/updater backend/nyanpasu-core/src/download` 退出码 1；`rg 'pub fn parse_version|pub enum CoreVersionError' backend/nyanpasu-core/src` 退出码 1；`rg 'client::core_version::(parse_version|CoreVersionError|RunCoreVersionSnafu|CoreVersionExitSnafu)' backend` 退出码 1。
- `reads_versions_without_mistaking_banner_words_for_versions` 在 contracts 下通过；updater、download、workflow 的测试名清单不变（只允许前缀变化）且通过；三个生成文件无 diff；门禁通过（contracts 仍不含其禁止项）。

## B4b — `nyanpasu-updater` 与注入的后端

- [x] 新建 `backend/nyanpasu-updater`：`C/updater/**` 与 `C/download/**` 整体移入（owner、`HttpUpdaterBackend`、下载与解包，V-14）。公开导出跨 crate 需要的项：`UpdaterClient`、`HttpUpdaterBackend`、`ports` 中的 `UpdaterBackend`、`CoreUpdateInstaller`、`CoreTypeMeta`（出现在 `UpdaterBackend::prepare`）与 B4a 的制品类型；core workflow 测试用的 `updater::tests::ReadyBackend`（`C/client/application_workflow/tests/mod.rs:927`）移到 updater 的 `test-support` 模块，不导出整个 tests 模块。
- [x] facade 不再构造 `HttpUpdaterBackend`（`C/client/mod.rs:508-518`）。updater crate 定义可失败的工厂：

  ```rust
  pub type BackendFactory = dyn Fn(Arc<dyn SelfProxyPortSource>)
          -> Result<Arc<dyn UpdaterBackend>, BackendFactoryError>
      + Send + Sync;
  ```

  `BackendFactoryError` 是模块自有 snafu 枚举，两个变体对应"取可执行文件路径失败"（source `io::Error`）与"可执行文件没有父目录"。`ClientSetupArgs` 增加 `Arc<BackendFactory>` 字段，facade 在原位置（会话端口源已存在、workflow 已构造之后）调用，把两个变体映射回现有的 `SetupError::LocateExecutable`、`LocateExecutableParent`（`:171-173`），不另加一层显示或 source。tauri `T/setup.rs` 的闭包在被调用时执行 `std::env::current_exe()` 与取父目录，再用 `PathResolver`、元数据构造 `HttpUpdaterBackend`。后端的 HTTP 客户端（UA、120 s 超时、有 mixed 端口时显式走 self proxy 否则保留 reqwest 默认系统代理，`C/updater/instance.rs:76-89`）原样随后端，不改用 net 的策略。

- [x] core 的 `Cargo.toml` 删除 `zip`、`tar`、`flate2`、`bolt-load`（`:33-35,63`）、`smol-cancellation-token` 及其他只被 updater、download 使用的依赖，updater crate 声明它们。
- [x] 调用方迁完：core 的 client updater API、workflow 安装实现、测试装配；tauri 的 `setup.rs`、`ipc.rs` 与 specta 导出（`ManifestVersionLatest`、`UpdaterState`、`UpdaterSummary`、`DownloaderState`、`DownloadStatus` 名字不变）。
- [x] 登记：workspace；`CORE_FAMILY` 加它（正反策略测试）；`STATIC_GATE_PREFIXES`；musl 脚本；`backend-packages.md`、`architecture.md`；路线图的 Delivered baseline 与 Phase B 状态。

**验证**

- `rg HttpUpdaterBackend backend/nyanpasu-core/src` 退出码 1；`backend/nyanpasu-core/Cargo.toml` 不声明 `zip`、`tar`、`flate2`、`bolt-load`、`smol-cancellation-token`（它们仍经 updater 传递进入，这是 V-14 的预期）。
- updater 29 例（`instance.rs` 19、`tests.rs` 9、`shared.rs` 1）加 B4a 移入的 `test_mirror_speed_test`，与 download 8 例的测试名清单移动前后一致且通过，含 `new_http_operation_uses_latest_actual_proxy_port`、`the_shutdown_waits_for_an_install_in_progress`、`the_shutdown_waits_for_an_extraction_in_progress`。
- 新增测试：工厂返回的两个错误分别得到原 `SetupError` 变体（`Display` 与 source 链）。
- 三个生成文件无 diff；门禁通过。
- Phase B 全部完成后，按 design §6 复测编译时间，与 A 结束的数据对比，写进 PR-B5 描述。

---

## Phase B–E 里程碑卡

每张卡开工前先细化为与 Phase A 同粒度的任务并审阅；B1a–B4 已细化为 §2 的卡。依赖图、决策、前置、不变量、完成判据见 design §5。

| 卡  | 内容                                                                                          | 前置           | 完成判据（摘要）                                                        |
| --- | --------------------------------------------------------------------------------------------- | -------------- | ----------------------------------------------------------------------- |
| A2b | 脚本扩展注入与引擎 feature                                                                    | A2             | design §5.0                                                             |
| B1a | persistence 收 format 与通用原子写；`session_state` 不依赖 migration                          | A5b            | 无 `session_state → migration`；两个恢复测试通过                        |
| B1b | backup/migration 的存储端口；migration crate                                                  | B1a            | migration crate 不依赖 redb                                             |
| B2  | facade 签名去具体存储类型；traffic legacy 导入器 feature 化                                   | B1b            | `ClientSetupArgs` 类型闭包无 `Storage`/redb；traffic 无默认 redb        |
| B3  | logs、traffic、geo 成 owner crate；`owner_shutdown` 按需建 crate                              | B2             | owner 生产代码不含 clash/core-control/redb/notify；traffic 回归测试通过 |
| B4  | updater crate；后端与其 HTTP 客户端由 shell 注入                                              | B3a            | facade 不构造 `HttpUpdaterBackend`；facade 无归档依赖                   |
| C1  | 展示能力可选、结构化不支持                                                                    | A              | 无展示能力的装配测试                                                    |
| C2  | `nyanpasu-os` 与提权同意端口；能力端口进 contracts；重审 `update_core` 的 WebUI 暴露（#6157） | A              | core 无 `runas`/`osascript`；WebUI 触发提权得到不支持                   |
| C3  | `nyanpasu-desktop`；共享收敛与展示效果分离                                                    | C1、C2         | 门禁禁止 core 族/server → desktop；效果与热键回归测试通过               |
| D1  | 状态引擎（约 7,000 行完整闭包）                                                               | B1a            | 引擎测试名清单不变且通过；不依赖 core 族其他成员                        |
| D2  | 反转 mutation 参与；运行时值下移；拆 core-control 与应用                                      | D1、B3、B4、C3 | 无具体 workflow client；workflow 取消/恢复/关闭测试通过                 |
| E1a | 宏中立声明、按目标 opt-in 与能力元数据；protocol 信封                                         | A              | protocol 无 core 族/Tauri；bindings 无 diff                             |
| E1b | 完整请求/响应 DTO 进 protocol                                                                 | E1a、D2        | protocol 类型闭包不经 core 族                                           |
| E2  | `nyanpasu-rpc` 注册表与事件总线                                                               | E1a            | 注册表无 Tauri 类型                                                     |
| E3  | 纯 API WebUI；local socket 传输                                                               | E2             | 无资源启动；未 opt-in 被拒；跨传输一致                                  |
| E4  | `nyanpasu-server`，然后 `nyanpasu-cli`                                                        | C3、E1b、E3    | CLI 不链接 core 族；同一 server 会话                                    |
| E5  | OpenWrt                                                                                       | 用户明确恢复   | 单独验收                                                                |

B 结束时按 design §6 复测编译时间。

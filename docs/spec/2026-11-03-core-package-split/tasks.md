# Core 包族拆分、Shell 与 RPC 目标 任务

关联设计：`design.md`（同目录）。路径缩写同 design §1。

## 0. 全局约束

1. 遵守 `AGENTS.md` 与 `docs/development/*`：不加全局单例；依赖显式注入；不给 core 族加 Tauri 依赖；`NyanpasuClient` 只是 facade。
2. 移动与保行为接缝只移动代码、注入依赖、收窄依赖方向。行为改动单独成卡（A2b、C1 的不支持结果、C2 的同意策略、HTTP 限制与 RPC 审计），不夹在移动提交里；同一 PR 内的卡也分别提交。行为卡的前端例外仅限 design §9；移动中发现的现存问题记在 PR 描述里，不顺手改。
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

## 3. Phase C 的 PR 划分

本节是 design §5.2 的细化，行号记录于 `e004ef566`（A、B 已完成），按符号定位；新文件、符号与测试均为计划项。偏差见 design §8.1 V-16–V-22，决策见 §8.2 O-5–O-9。全局约束 §0 适用；本次文档提交是 PR-C0，不是实施卡。下表每张卡一个可独立编译、测试通过的提交，箭头表示提交顺序。

- 新 crate 只有 `nyanpasu-os`，为实现 crate；不建 `nyanpasu-desktop`。新增包同时登记 `S/backend-boundaries.ts`、`backend-boundaries_test.ts`、`S/policy.ts` 的 `STATIC_GATE_PREFIXES` 与覆盖测试，以及 `scripts/src/testing/test-backend-musl.ts:68-69,77-88` 的 `-p` 列表和 `expectedTargets`。新增集成测试目标也登记，不能只加库目标。
- 下文 `K/` = `backend/nyanpasu-contracts/src/`，`U/` = `backend/nyanpasu-updater/src/`，`O/` = 新建的 `backend/nyanpasu-os/src/`，`D/` = 新建的 `T/desktop/`；其余缩写同 design §1。`rg` 用显式文件参数或 `--glob`，不用 shell 花括号；退出码 1 才算无匹配，2 是检查失败。检查包含测试时迁完测试导入；生产限定检查必须列明文件，不能用排除 `tests.rs` 冒充排除了内联测试。
- 所有卡执行 §0.7；下列 Rust 命令从 `backend/` 运行，`pnpm run` 从仓库根运行。测试名清单按 §0.8 比较，新增测试单独列出；平台互斥测试分目标比较。触及 Unix/macOS 分支补 `cargo check -p <受影响包> --target x86_64-unknown-linux-gnu`，macOS 分支只在 macOS CI 验证，Linux 交叉检查不能替代它。
- bindings 统一通过 `cargo test -p clash-nyanpasu export_typescript_bindings` 重新导出（`T/specta_export.rs:444-452,484`）；纯移动与接缝三份生成文件零 diff。行为卡只允许所列类型变化，不改命令参数/返回 DTO 的其他字段。
- 行为卡新增消息放 `frontend/nyanpasu/messages/{en,ja,ko,ru,zh-cn,zh-tw}.json`（这里只是路径集合，不作 shell 展开）；遵守 `docs/development/typescript.md` 的消息分组，通过 `pnpm run web:build` 的 Paraglide 插件重新编译（`frontend/nyanpasu/vite.config.ts:74-76`），不手改 `src/paraglide/`。分别运行 `pnpm --filter @nyanpasu/rpc typecheck`、`pnpm --filter @nyanpasu/query typecheck`、`pnpm --filter @nyanpasu/nyanpasu typecheck`，不得用根级 `pnpm typecheck` 的无输出代替包检查。
- 文档同步：创建 OS 的卡更新 `backend-packages.md` 包表和 `architecture.md` 布局；完成能力、OS、桌面迁移的卡分别更新路线图的状态。没有新增 crate 的卡不新增门禁集合、静态前缀或 musl 包；代码迁移到已有 tauri 前缀后仍受静态门禁覆盖。

| PR    | 卡              | 依赖                   | 说明                                                                                |
| ----- | --------------- | ---------------------- | ----------------------------------------------------------------------------------- |
| PR-C0 | 本次文档细化    | A、B                   | 记录决策与卡，不实施代码                                                            |
| PR-C1 | C1a → C1b       | A、B                   | 先切共享/展示接缝，再改变缺失能力行为；两个提交                                     |
| PR-C2 | C2a             | A、B                   | contracts 端口与完整调用闭包；可与 PR-C1 并行                                       |
| PR-C3 | C2b → C2c       | C2a                    | 先建 OS 并移无提权机制，再移服务/DNS/安装；不改变授权行为                           |
| PR-C4 | C2d → C2e → C2f | C2c                    | 同意策略、最小 HTTP 修复、审计切面分别提交；不加逐调用权限上下文                    |
| PR-C5 | C3a             | C1a（建议 C1b 后合入） | 桌面机制整体移到 tauri；不依赖 C2，hotkey owner/main-thread/error 同迁，解析留 core |

---

## C1a — 共享效果与展示机制的消费接缝

本卡是保行为接缝，不改变能力是否可用或任何错误结果。

- [x] 拆 `C/client/effects/executor.rs:40,204` 的 `ApplicationEffectExecutor`：共享执行器保留 `Logger`、`SystemProxy`/`ProxyGuard`/`AutoLaunch`，`CoreLogCaptureEffects`（`:268`）仍处理 core 日志；展示执行器 `PresentationEffectExecutor` 暂放 `C/client/effects/presentation.rs`，收 `apply_locale`、`apply_hotkeys`、`apply_widget`、`apply_widget_logo`、`apply_tray` 与 `tray_full_pending`。core 在 `effects/ports.rs` 定义 `PresentationEffectsPort`，传既有 `EffectRevision` 和只含展示项的 `ApplicationEffectPlan`，返回每项一个 `EffectStatus`；不复制另一套 plan/status，不加逐项能力查询。共享执行器按原循环位置传单项展示 plan（不能先批量执行全部展示项再执行 Logger）。现有私有 `healthy`/`degraded`（`executor.rs:369,378`）移到 `effects/status.rs`，本卡设为 `pub(crate)`，两执行器共用；C3a 跨 crate 时再公开。
- [x] 保留 `EffectKind` 排序、locale 先于 tray（`executor.rs:216-219`）、失败的 full 刷新覆盖后来 part（`:174-188`）；共享执行器在原位置路由，不能把 `Logger` 从视觉组中提前或移到只在桌面存在的分支。`effects/actor.rs:138-149` 的四组、修订、源切片、重试、过期过滤与 receipt 原样，不能串行化独立组。
- [x] `HotkeyBindings::parse`（`C/client/hotkey/ports.rs:199-229`）的形状、canonical、重复键检查和 `AcceleratorValidator`（`:315`）留 core；`C/client/mod.rs:970-974` → `hotkey::validate_bindings`（`hotkey/mod.rs:140`）的提交前校验不变，不新增列表验证端口。tauri 已依赖 core，C3a 的 desktop 直接调用这些公开项。纯 diff `HotkeyBindings::diff`/`HotkeyOp`（`ports.rs:256,170`）也留 core；owner 按 diff 执行注册的机制才移到 desktop。
- [x] `T/setup.rs:676-727` 继续传全部真实能力；`T/adapters/hotkey.rs:28` 的 `PlatformAcceleratorValidator` 继续实现既有端口。迁完 `C/client/test_support.rs:797`、`test_support/facade.rs`、内联 `ClientSetupArgs`、`T/adapters/hotkey/tests.rs:22` 以及效果测试的构造方，不留旧参数转发层。`ApplicationEffectsPort` 仍是共享收敛 owner 的内部端口，不能整个变成可选而丢掉共享效果。
- [x] Cargo 不新增包或依赖；tauri → core、已有 config 边继续 `default-features = false`。门禁集合、`STATIC_GATE_PREFIXES`、musl 列表均不变；同步本卡接缝在路线图的描述，不声称 C3 已完成。

- [x] **先写回归，再拆 executor**：在 `T/adapters/ui_effects/tests.rs` 新增 `failed_full_refresh_is_retried_until_success_then_part_stays_part`：full 失败 → 请求 part 实际调用 full 并成功 → 再请求 part 实际只调用 part；断言调用顺序为 full/full/part、各次次数及状态依次为 TrayRefreshFailed 的 Degraded/Healthy/Healthy，并核对 revision。保护 `executor.rs:174-188` 的 pending 生命周期；既有 `late_full_tray_refresh_still_runs_after_a_newer_part_refresh:663` 两次都成功，单独保留其乱序覆盖，不能代替失败路径。
- [x] 在 `T/adapters/hotkey/tests.rs` 新增 `duplicate_accelerators_are_rejected_before_commit`，复用 `:91` 的真实 facade/PlatformAcceleratorValidator 与 effects mock；提交不同 action 的 Ctrl+Q/Control+Q，断言 DuplicateAccelerator、磁盘配置字节与读取配置均不变、effects apply 从未调用。原 `C/client/hotkey/tests.rs:709` 只覆盖缺 modifier，不声称它覆盖重复键。

**验证**

- `cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；按 §0.8 比较 hotkey 18 例、effects 18 例、tauri `adapters/ui_effects/tests.rs` 21 例的清单，清单另加本卡两个新测试，只允许实现模块前缀变化。既有 `locale_is_applied_before_tray_refresh`（`T/adapters/ui_effects/tests.rs:236`）、`late_full_tray_refresh_still_runs_after_a_newer_part_refresh`（`:663`）、`blocked_pac_does_not_block_visual_or_hotkey_group`（`C/client/effects/tests.rs:136`）、`invalid_hotkey_is_rejected_before_commit`（`C/client/hotkey/tests.rs:709`）通过。
- `rg 'HotkeyClient|WidgetController|TrayRefresher|LocaleSink' backend/nyanpasu-core/src/client/effects/executor.rs` 退出码 1；展示实现中允许这些消费。`rg -n 'validate_bindings|HotkeyBindings::parse' backend/nyanpasu-core/src/client/mod.rs backend/nyanpasu-core/src/client/hotkey/mod.rs` 正向核对原提交前调用链；解析实现仍在 core，不删校验。
- 反向检查：调换 locale/tray 或在 full 失败后清掉 pending，分别使 locale 顺序测试与新增 `failed_full_refresh_is_retried_until_success_then_part_stays_part` 失败；把 core 解析器的重复键检查删掉，`parse_rejects_duplicate_accelerator`（`C/client/hotkey/tests.rs:77`）及新增 `duplicate_accelerators_are_rejected_before_commit` 失败。bindings 三份零 diff。

## C1b — 缺失展示能力的结构化不支持

本卡是行为变更：无展示能力可装配；显式热键编辑在提交前拒绝，既有期望的展示效果报告不支持而不是已应用。

- [x] `ClientSetupArgs`（`C/client/mod.rs:135-137`）的 window、`accelerators` 变为 `Option<Arc<dyn ...>>`；共享执行器持有 `Option<Arc<dyn PresentationEffectsPort>>`，展示端口整体可缺失，不加逐项 capability 查询。字段文档约定：shell 当且仅当传入会注册热键的 presentation executor 时传 `accelerators`；存在的展示执行器仍持有完整 validator/registrar 等依赖。显式热键编辑仅在 `accelerators` 为 None 时提交前拒绝；validator 存在但 presentation 缺失属于装配错误，不增加专门检测或支持分支，效果仍走整体缺失的 Unsupported。已存在 owner 停止保留原 stopped 错误。`ClientHostAdapters.debug_http`（`C/client/host_adapters.rs:49`）变可选；缺失时不调用工厂、不登记 HTTP shutdown 任务，存在时保留 `C/client/mod.rs:434-442` 的等待和 `T/server/debug_http.rs:230-233` 的 disable 后 drain。主线程执行器不是 setup 字段（`T/setup.rs:205`），只由桌面机制持有。
- [x] 显式 hotkey patch（包括空列表）在能力缺失时返回新增 `ConfigError::ValidateHotkeyCapability`（wire `validate_hotkey_capability`，无字段），不写配置；不涉及 hotkeys 的 patch 原样。启动、导入、已有期望值走 `EffectHealth::Unsupported`。缺失整个展示端口不是 `HotkeyStopped`；没有验证器不是 `UnsupportedAccelerator`。验证器存在时仍执行原 `validate_bindings`，不改变解析规则。更新 `C/client/hotkey/mod.rs:140`、`C/state/config_error.rs:35`、`T/ipc.rs:1718` 的完整路径。
- [x] `C/client/effects/status.rs:36,100` 复用 `Unsupported { code }`，只新增 `EffectFailureCode::PresentationUnsupported`（wire `presentation_unsupported`）；展示端口缺失时 Locale、Hotkeys、Widget、WidgetLogo、Tray 每项均返回此 code，状态已有 kind 区分效果，不新增 health 枚举。`effects/actor.rs:395-426` 只让 Healthy 推进 applied_revision 的规则原样；Unsupported 不自动重试。显式 `retry_effect_now`、tray/logo refresh 保留“owner 接收入队”的返回语义，最终不支持通过状态查询，不加 `EffectsError` 变体。
- [x] 新增 `WindowError::ToggleDashboardUnsupported`（非 wire 错误）；在 `C/client/debug_http.rs` 新增 `DebugHttpControlError::{ReadStatusUnsupported, ChangeEnabledUnsupported, ReadStatus, ChangeEnabled}`，后两项保留原 anyhow source 但序列化跳过 source，错误按 `kind` snake_case。`C/client/mod.rs:692-699` 的 debug 查询/开关与 `T/ipc.rs:2492,2500` 无能力时分别返回前两项；`shutdown_debug_http` 无能力时成功，有能力时保留原 disable 行为。`T/ipc.rs:64,118` 增加 `IpcErrorKind::DebugHttp` 及映射；不能把类型错误重新压成 unknown 文本。
- [x] `UiEventSink` 保持必需的中立通知端口，shell 可传事件总线/null；`FrontendLogSink` 是输入诊断到 tracing（`C/client/frontend_events.rs:96-105`，`C/client/logs.rs:20`），保持必需的 tracing/null。app-update factory/sink（`C/client/mod.rs:125-126,544-552`）保持现有 optionality 与现有错误行为，本卡不扩大 app updater 的错误模型。
- [x] 改 `C/client/test_support.rs:752-800,918-933`、`test_support/facade.rs`、`client/mod.rs` 内联构造、`T/test_support.rs:49` 与 `T/setup.rs`：新增真正缺失能力的装配，不用 `NoopApplicationEffects` 伪造 Healthy。新增测试（均为新）：`headless_client_constructs_without_presentation_capabilities`、`missing_hotkey_capability_rejects_before_commit`（`accelerators = None`，覆盖非空与空列表；不为不合法的 validator/presentation 组合新增支持逻辑）、`unrelated_config_patch_does_not_require_hotkeys`、`missing_window_returns_unsupported`、`missing_debug_http_returns_structured_unsupported`、`unsupported_presentation_never_advances_applied_revision`（断言五种展示 kind 均为 `presentation_unsupported`）、`unsupported_presentation_does_not_retry`、`headless_shared_effects_still_converge`、`headless_shutdown_without_debug_owner_completes`。
- [x] 仅改 `frontend/nyanpasu/src/utils/ipc-error.ts` 的 `configErrorMessage`、`ipcErrorMessage` 和 `effectFailureMessage` 分支，新增 debug 错误 mapper；新增消息 `error_config_hotkey_capability_unsupported`、`error_debug_http_read_status_unsupported`、`error_debug_http_change_enabled_unsupported`、`error_debug_http_read_status`、`error_debug_http_change_enabled` 与一个 `effect_failure_presentation_unsupported`，六种语言齐全；不改页面/组件。按本节公共流程重新编译 Paraglide。
- [x] 无新 crate，Cargo/门禁集合/静态前缀/musl 注册不变；端口 mocks 由 core `test-support` 提供，tauri dev 边保持关闭 core 默认 feature。同步路线图 C1 状态及中立 sink 说明。

- [x] 补齐赋值点：`C/client/hotkey/tests.rs:581` 的 args.window、`T/adapters/hotkey/tests.rs:27` 的 args.accelerators、`T/unified_rpc.rs:1090,1209` 的 args.host_adapters.debug_http 均保留真实能力并包 Some；HTTP fixtures 仍用真实 enabled owner，保留 shutdown drain，不能换成 None/no-op。
- [x] 前端行为测试属于本卡允许范围：在 `frontend/nyanpasu/tests/ipc-error.browser.test.ts` 新增 `hotkey_capability_error_uses_its_message`，经 ipcErrorMessage → configErrorMessage 断言 `m.error_config_hotkey_capability_unsupported()`；新增参数化 `debug_http_errors_use_their_messages`，四个变体经 ipcErrorMessage → debug mapper 分别断言 `m.error_debug_http_read_status_unsupported()`、`m.error_debug_http_change_enabled_unsupported()`、`m.error_debug_http_read_status()`、`m.error_debug_http_change_enabled()`；新增 `presentation_unsupported_uses_its_message`，调用 effectFailureMessage 断言 `m.effect_failure_presentation_unsupported()`。除 typecheck 外运行 `pnpm run test:frontend`（根 package.json:49，Vitest browser；见 testing.md:39），不能用编译替代行为断言。消息路径以 `frontend/nyanpasu/project.inlang/settings.json:10` 和实际 `frontend/nyanpasu/messages/` 为准。

**验证**

- §0.8 清单 = C1a 清单 + 本卡新增测试；`cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；既有 `stale_completion_does_not_publish_success_for_new_desired`（`C/client/effects/tests.rs:304`）、`commit_receipt_and_status_keep_source_separate_from_pending_notifications`（`:803`）、`enabled_http_server_joins_application_shutdown`（`T/unified_rpc.rs:1074`）通过。
- `rg 'expect\(".*(window|presentation|debug HTTP)' backend/nyanpasu-core/src/client/mod.rs backend/nyanpasu-core/src/client/effects/executor.rs` 退出码 1，另逐项审阅 optional 分支；不能仅用搜索证明所有分支已处理。反向检查：缺失能力改用 `NoopApplicationEffects`、校验移到提交后或忽略 HTTP drain，分别使 applied_revision、未持久化、shutdown 测试失败。
- bindings 只新增 `ConfigError.validate_hotkey_capability`、`EffectFailureCode.presentation_unsupported`、`DebugHttpControlError` 四个变体与 `IpcErrorKind.debug_http`；`WindowError` 不导出，`EffectsError`、app-update DTO、命令签名不变。逐包运行本节列出的 rpc/query/nyanpasu typecheck；新增错误的 serde、Display/source 链测试。没有其他前端改动。

---

## C2a — contracts 的 OS 消费端口与闭包

本卡是保行为接缝；先拆闭包，具体实现仍留原 crate。

- [x] `SystemDnsCache/SystemDnsError`（`C/client/system_dns.rs:11-36`）移到 `K/system_dns.rs`；`OsProxyConfig/OsProxyError/OsProxyPort`、`AutoLaunchError/AutoLaunchPort`、`PacError/PacPort`（`C/client/system_proxy/ports.rs:15-194`）移到 `K/system_proxy.rs`；`CoreVersionReader`（`C/client/core_version.rs:7`）进已有 `K/core_version.rs`。生产选择器无条件公开：DNS 与 installer 的 `#[snafu(visibility(pub(crate)))]`（`C/client/system_dns.rs:9`、`core_lifecycle/ports.rs:19`）改为 `pub`，包括 RunFlushCommand/FlushRejected/Unsupported 与 PathNotUtf8/StartElevatedCopy/ElevatedCopyFailed selectors；仅 mocks、测试构造辅助放在 `test-support`，不保留旧 re-export。`SystemProxySource` 已在 `K/net.rs:61`，不再搬一次。
- [x] 二进制端口使用窄请求：`BinaryInstaller` 与 `InstallCoreBinaryError`（`C/client/core_lifecycle/ports.rs:20,45`）进 `K/core_install.rs`；新增 `CoreBinaryInstallRequest` 包含 `ClashCore`、source/destination 路径、`Arc<TempDir>` staging lease、可选 companion 的 staged/destination 路径，不包含进度回调、restart 或 updater 类型。`PreparedCoreBinary/PreparedCoreCompanion/BinaryInstallProgress` 留 `U/updater/artifact.rs:8-38`；workflow 在 `C/client/core_lifecycle/workflow.rs:715` 由借用的 artifact 构造请求，持有 artifact/companion 直到安装返回；阻塞复制任务照旧克隆并持有 lease（`adapters.rs:46-61`），不能退化为只有裸路径。这样不搬 B4 的进度闭包，也不产生 OS → updater。
- [x] `ServiceHostAdapter`（`C/actor_v2/service_actor.rs:44-52`）拆为 contracts 的 `ServiceProcessControl`（async `probe -> Result<String, ServiceCommandError>`，install/uninstall/start_daemon/stop_daemon/update）及 core 内单独注入的 endpoint factory。OS probe 负责进程退出与 UTF-8；core 在同一 probe 调用边界解析 `StatusInfo` 并构造原 `ParseServiceStatus`。`ServiceEndpoint`/`EndpointHandle` 与 IPC 解释留 core；替换 actor args、状态与测试 fake，不能留下一个转发所有方法的旧 `ServiceHostAdapter` 兼容壳。
- [x] `ServiceCommand/ServiceCommandError`（`C/service/control.rs:18,40-105`）进 `K/service.rs`：保留 platform `UserError`、`InstallDirError`、IO/UTF-8/JSON source、退出码/signal/timeout 字段、serde skip 与 Display。contracts 可依赖所需 `whoami`/`nyanpasu-paths`，不依赖 runtime IPC 客户端。先加真实 source 的 Display/source 链/JSON 期望测试，再改导入与解析位置。不把 core `RuntimePreparationPort`、`CoreSpec` 或 runtime 状态移入 contracts。
- [x] 为后续纯移动提前参数化 Tauri executable lookup：`T/adapters/system_proxy.rs:78` 的 autostart 在原 setup 时点由 shell 解析 executable，再传入配置解析函数；`T/utils/core_version.rs:19` 必须仍在每次 `read` 时查询，不能提前变成 setup 错误。为后者在 `K/core_version.rs` 新增 `CoreVersionExecutableSource: Send + Sync + 'static`，同步 `current_exe(ClashCore) -> Result<PathBuf, CoreVersionError>`，由 `T/adapters/core_version.rs` 调原 Tauri API 并保留 `RunCoreVersion` source，注入版本探测实现。原 Windows 引号、macOS `.app`、Linux AppImage、测试 `deps` 目录处理、`verge-dev` 不变；不换 executable discovery API。
- [x] 调用方同提交迁完：`C/client/{mod,core_version,system_dns}.rs`、system_proxy actor/tests、core_lifecycle workflow/adapters、`client/test_support.rs`、`test_support/{facade,workflow}.rs`、application_workflow tests 的 `mod/startup/connection_policy/mutations.rs`、`actor_v2/{service_actor,service_host_adapter}.rs` 及其 fake、`T/{setup,ipc,test_support}.rs`、`T/adapters/system_proxy.rs`、`T/utils/{core_version,collect}.rs`。updater 的 artifact 定义和公开路径不变。
- [x] contracts → config、tauri → core/config 及新增的对应边均 `default-features = false`；contracts 加 `tempfile`、新增端口使用的 `tokio-util`；保留已有 `url`（`backend/nyanpasu-contracts/Cargo.toml:26`）等实际依赖，test-support 转发 mockall。`S/backend-boundaries_test.ts` 增加 contracts → updater/core-manager/reqwest（含 IPC client 传递引入 reqwest）的负例和 contracts → config/paths 的正例，规则本身已有禁止项，不放宽。既有 contracts 静态前缀不变；musl 同时加 `-p nyanpasu-contracts` 与 `lib:nyanpasu_contracts`，覆盖迁入测试。同步 backend-packages 的 contracts 职责。

- [x] 补齐原路径消费者：`C/client/application_workflow/mod.rs:33,190`、`runtime_error.rs:19,23`、`system_proxy/mod.rs:21`、`system_proxy/error.rs:9`、`C/actor_v2/facade.rs:924,1060`、`actor_v2/test_support.rs:76,194`、`client/application_workflow/tests/recovery.rs:615,762`、`tests/service_recovery.rs:5,65`、`C/service/test_support.rs:3`、`T/setup_tests.rs:20,22,78`、`T/adapters/ui_effects/tests.rs:42-44`。每个 service fake 同提交提供分离的 process-control 与 endpoint 依赖；UI-effect mocks 本卡改从 contracts 导入，不等 C2c/C3a。对 core/tauri 的全部 rs 再搜 ServiceHostAdapter、ServiceCommandError、BinaryInstaller、InstallCoreBinaryError、CoreVersionReader、SystemDnsCache 及分组 system_proxy ports 导入，逐项清零旧路径。
- [x] 正常生产检查不得依赖测试 feature：`cargo check -p nyanpasu-contracts -p nyanpasu-core -p clash-nyanpasu --lib`，不传 --all-features/--features test-support/--all-targets；C2b/C2c 创建 OS 后同样 `cargo check -p nyanpasu-os --lib`。反向把生产 selector 藏入 test-support 必须使正常消费端检查失败；不能只靠全 feature 测试。

**验证**

- `cargo test -p nyanpasu-contracts`、`-p nyanpasu-core`、`-p nyanpasu-updater`、`-p clash-nyanpasu`；§0.8 四侧清单合计 = 原清单 + 新增 `service_error_keeps_display_source_and_wire_shape`、`os_port_errors_keep_display_source_and_wire_shape`、`install_request_keeps_staging_when_waiter_is_dropped`、`version_probe_resolves_executable_on_each_read`、`version_probe_resolution_failure_stays_a_read_error`、`autostart_path_parameterization_preserves_platform_paths`（均为新测试），只有端口测试迁入 contracts；既有 `the_io_error_stays_out_of_the_wire_form_but_in_the_message`（`C/service/control.rs:342`）、`reads_versions_without_mistaking_banner_words_for_versions`（`K/core_version.rs:59`）保持。
- `rg 'nyanpasu_updater|nyanpasu_core_manager|nyanpasu_ipc|reqwest' backend/nyanpasu-contracts/src/core_install.rs backend/nyanpasu-contracts/src/service.rs` 退出码 1；`rg 'ServiceHostAdapter' backend/nyanpasu-core/src backend/tauri/src` 退出码 1（含改名后的具体实现）；`rg 'client::core_version::CoreVersionReader|client::system_proxy::ports::(OsProxyPort|AutoLaunchPort|PacPort)' backend/tauri/src` 退出码 1，并审查分组导入。
- 反向检查：把 artifact 参数换回 updater 类型或 contracts 加 IPC client 依赖，门禁失败；把 source 装箱改为字符串，契约测试失败；去掉 staging lease，新增 `install_request_keeps_staging_when_waiter_is_dropped` 失败。bindings 三份零 diff。
- `cargo check -p nyanpasu-core -p nyanpasu-contracts -p clash-nyanpasu --target x86_64-unknown-linux-gnu`；macOS CI 检查平台 source/路径分支；`pnpm run test:scripts`、`pnpm run test:backend-musl`。

## C2b — 新建 `nyanpasu-os`，移动代理与设备机制

本卡是纯移动；C2a 已完成构造参数化，不改变代理、PAC、自启动与版本探测策略。

- [x] 新建 `O/system_proxy_source.rs`、`device_identity.rs`、`system_proxy.rs`、`autostart.rs`、`pac.rs`、`core_version.rs`，分别接收 `C/utils/config.rs:16`、`C/utils/hwid.rs:29,48`、`T/adapters/system_proxy.rs:30,69,122,183`、`T/utils/core_version.rs:10` 的实现。autostart 的 `verge-dev` 语义由 shell feature 显式转发；PAC 的 timeout、大小限制、缓存失败策略、取消和 disable 行为原样。core 的系统代理 owner 留 core。
- [x] OS 内放私有 `blocking::join`，复制 `backend/nyanpasu-owner-support/src/blocking.rs:6-14` 的结果处理：正常返回、panic 原 payload `resume_unwind`、runtime 取消 panic。它只供迁入的 PAC/服务/安装机制使用，不新增共享 crate，不依赖 owner-support。保留原 spawn_blocking 与等待位置，避免改同步/异步 API 或 caller-drop 语义；新增 `blocking_join_resumes_the_original_panic`、`blocking_join_panics_on_runtime_cancellation`（新测试）。
- [x] 调用方改为 shell 构造 OS 实现：`T/setup.rs:293,342,385,415,429,469,485,701,703`、`T/utils/collect.rs:49`、`T/test_support.rs:24`。core 的 `profile_integration_tests/test_support.rs:25`、`profiles.rs:784,1016` 改用显式注入的 contracts proxy fake，不能让 core 导出的 test-support 链接 OS。删除旧实现模块/导入，不留 shim。
- [x] workspace `members` 加 OS；tauri 普通依赖 OS，OS 只依赖 foundation/中立工具与实际第三方库，不依赖 core 族、net 或其他实现 crate；所有 config/core 边关默认 feature。OS 声明 `sysproxy`、`auto-launch`、`sysinfo`、`whoami`、`once_cell`、`sha2`、`hex`、Windows `winreg`；PAC 保留自己的 `reqwest`（沿用 workspace TLS/features）、`url`、`tokio-util`，不借 net；其余直接依赖为 `anyhow`、`async-trait`、`camino`、`dunce`、`serde`/`specta`（DeviceInfo）、`snafu`、`tokio`、`tracing`，版本沿用原 manifest（core `Cargo.toml:78-110`、tauri `Cargo.toml:114-116,127,151-160`）；SHA-256/hex 仍被 core runtime/icon/geo 使用，不能误删。core 不加 OS 普通依赖或 test-support 转发。
- [x] `S/backend-boundaries.ts:40-44` 的 `IMPLEMENTATION` 加 OS，继承 GUI 禁止及双向 core 族禁止、实现互依禁止。`backend-boundaries_test.ts` 正例 shell → OS、OS → contracts/config；负例逐个 core 族 → OS、OS → 各 core 族（含 updater/owner-support）、OS → net/profile-io/scripts、foundation → OS、OS → Tauri/egui/Slint（含传递路径）。不要把不存在的 server 当现有 cargo tree 根。
- [x] 在删除 core 最后一个 `sysproxy` 使用与 `backend/nyanpasu-core/Cargo.toml:78` 后，扩展 `S/backend-boundaries.ts:111-114` 的 CORE_FAMILY 禁止项，加 `sysproxy`。`backend-boundaries_test.ts` 新增逐个 core 族直接/传递 → sysproxy 的负例及 OS/tauri → sysproxy 的正例；不禁止 OS 机制依赖它。
- [x] `S/policy.ts:81` 加 `backend/nyanpasu-os/src/`；`:169` 的 HWID immutable allowlist 改到 `O/device_identity.rs`，同步 `architecture-ledger_test.ts` 的覆盖测试；不新增可变服务 static。musl `-p nyanpasu-os`、`lib:nyanpasu_os` 同加。更新 backend-packages、architecture 的 OS 布局与路线图进度。

- [x] 删除 `T/ipc.rs:395-396` 所在的已注释 get_device_info wrapper：移动使其 nyanpasu_core::utils::hwid 路径失效，这是本卡明确追踪的删除，不是顺手清理；不为注释豁免旧路径 rg 检查。

**验证**

- `cargo test -p nyanpasu-os`、`-p nyanpasu-core`、`-p clash-nyanpasu`；§0.8 比较三侧清单。移动 HWID 五例：`test_hwid_is_deterministic`、`test_device_os_not_empty`、`test_os_version_not_empty`、`test_device_model_not_empty`、`test_fallback_hwid_is_valid_hex_32`（`C/utils/hwid.rs:218,226,233,239,245`）；其余源实现无本地测试，不能声称移动了 proxy actor 测试。既有 `pac_unsupported_platform_reports_unsupported`（`C/client/system_proxy/tests.rs:605`）、`exit_restores_captured_original`（`:751`）、`the_token_releases_a_pac_download_before_the_restore`（`:955`）留 core 并通过。
- `rg 'OsSystemProxySource|DEVICE_INFO|SidecarCoreVersionReader' backend/nyanpasu-core/src` 退出码 1；`rg 'nyanpasu_core::utils::(config|hwid)|crate::utils::core_version' backend/tauri/src` 退出码 1；`rg 'nyanpasu_(core|updater|owner_support|net)::' backend/nyanpasu-os/src` 退出码 1。bindings 三份零 diff。
- 反向检查：OS 加 owner-support/net 依赖、core 加 OS 普通依赖，门禁失败；core 加回 sysproxy 依赖，新增依赖规则失败；删 OS 静态前缀，覆盖测试失败；把 `resume_unwind` 换为普通错误，新增 panic 测试失败。`pnpm run test:scripts`、`pnpm run test:architecture-ledger`、`pnpm run test:backend-musl`；`cargo check -p nyanpasu-os -p clash-nyanpasu --target x86_64-unknown-linux-gnu`，macOS CI。

## C2c — 服务、DNS flush 与二进制安装机制移入 OS

本卡是纯移动，仍保留现有直接 OS 授权行为；同意策略在 C2d 单独改变。

- [x] `C/service/control.rs:124-299` 的进程机制与 C2a 的具体 process-control 实现移到 `O/service.rs`；`C/utils/sudo.rs` 移到 `O/elevation/macos.rs`；`C/client/system_dns.rs:40-104` 的 OS 实现移到 `O/system_dns.rs`；`C/client/core_lifecycle/adapters.rs:14-178` 的安装、companion 交换和错误进 `O/core_install.rs`，按 C2a 窄请求接收参数。改用 C2b 私有 blocking join，调用位置不变。
- [x] core 留 `ServiceEndpoint` factory、probe JSON 解释、compat policy/actor、runtime preparation、安装前 stop/recover 与安装后 restart/progress（`C/client/core_lifecycle/workflow.rs:704-723`）。不整体搬 `C/service/`，不搬 runtime DNS guard；`C/actor_v2/local_host.rs:69-74` 的既有 runtime controller 装配及 runtime submodule 内部结构本轮不变。
- [x] `T/setup.rs:263,279,478,483`、`T/ipc.rs:1645` 改从 OS 构造机制/生成服务命令参数；core 的 installer fixtures（`C/client/test_support.rs:791`、`test_support/facade.rs`、`client/mod.rs`、application_workflow tests 的 `mod/startup/connection_policy/mutations.rs`）改用已有 `test_support/workflow.rs` 的 fake，真实安装测试随 OS。所有旧模块声明与导出删除。
- [x] OS 加 `runas`、所需 Windows session 检查 API；Windows SID 继续用 `nyanpasu-utils` 的 `os` feature（`backend/nyanpasu-runtime/crates/nyanpasu-utils/Cargo.toml:93`），以直接 path 边 `nyanpasu-utils = { path = "../nyanpasu-runtime/crates/nyanpasu-utils", default-features = false, features = ["os"] }` 声明，不换 SID 算法。workspace 的既有项未关闭默认 feature（`backend/Cargo.toml:58`），不能用 `workspace = true` 再假设子边可以关闭它，也不扩大本卡修改所有旧消费者。该中立工具不引入 runtime IPC/core-manager；OS 不需要这两者，`S/backend-boundaries.ts` 为 OS 加禁止项与对应负例，保留 OS → utils 的正例；仅在 core 最后一个使用者消失后删除依赖。tauri 的 `runas` 仍被 UWP/迁移使用（`T/core/win_uwp.rs:34`、`T/utils/init/mod.rs:412`），不能删除。不加 OS → updater/owner-support/net 或 contracts → IPC client。macOS helper 加 `tempfile`，原 Unix `ExitStatusExt`/Windows session 分支原样；已有 OS 集合/prefix/musl 注册不变，删除 core 最后一个 `runas` 使用与 `backend/nyanpasu-core/Cargo.toml:70` 后，在 `S/backend-boundaries.ts:111-114` 的 CORE_FAMILY 禁止项加 `runas`；`backend-boundaries_test.ts` 加逐个 core 族直接/传递 → runas 的负例及 OS/tauri → runas 的正例。不新增源级扫描规则；授权站点只作下列 PR-time `rg` 审阅；更新路线图 C2 机械迁移状态。

- [x] 服务机制闭包还包括 `C/service/control.rs:108` 的 signal 与 `:121` 的 SERVICE_BINARY，随机制移入 OS。
- [x] **本卡负责安装测试 fixture 改写**：`C/client/core_lifecycle/adapters.rs:184-207,222-228` 不原样复制 updater 类型；迁入 `O/core_install.rs` 后，调用安装器的 `the_companion_is_published_once_the_binary_is_installed` 构造 `CoreBinaryInstallRequest`；其余四例照旧直接调用 `publish_companion` 并保留其错误断言（安装器对 companion 失败只记日志并返回成功，`adapters.rs:22-37`）。凡用到 updater 制品的 fixture 都改为本地 `TempDir` 所有者加 staged/destination 路径，删 NoProgress 与 BinaryInstallProgress/PreparedCoreCompanion 导入。C2a 改接口时先让原位置测试构造窄请求以保证该提交编译；本卡移入 OS 时完成去 updater 的本地 fixture 闭包。保留 `:239-246` 的 drop 后清理断言：释放 request/staging lease 与 companion owner 后目录只剩 installed-core/meow-alpha，不因 fixture 长持有 TempDir 而放宽断言。

**验证**

- §0.8 清单按 C2a 错误测试的最终归属比较。源库存：service 三例 `a_failed_command_names_the_command_and_how_it_ended`、`the_service_binary_sits_next_to_the_executable`、`the_io_error_stays_out_of_the_wire_form_but_in_the_message`（`C/service/control.rs:309,329,342`；错误测试可在 contracts）；DNS 两个平台互斥例 `windows_flush_uses_ipconfig`、`macos_flush_covers_both_resolver_caches`（`C/client/system_dns.rs:123,134`）；安装五例 `the_companion_is_published_once_the_binary_is_installed`、`a_first_companion_is_published_where_none_was`、`the_previous_files_left_by_an_interrupted_swap_are_restored_or_removed`、`a_failed_swap_restores_the_previous_files`、`a_linked_destination_is_left_alone`（`adapters.rs:214,250,261,285,303`）。
- `cargo test -p nyanpasu-os`、`-p nyanpasu-contracts`、`-p nyanpasu-core`、`-p nyanpasu-updater`、`-p clash-nyanpasu`；保留 `uninstall_stops_the_daemon_before_removing_it`（`C/actor_v2/service_actor.rs:1724`）、`command_settled_waits_out_a_running_helper_and_reaps_it_with_one_probe`（`:1581`）、`daemon_crash_reconnects_and_restores_only_a_previously_running_core`（`C/client/application_workflow/tests/service_recovery.rs:254`）、`the_shutdown_waits_for_an_install_in_progress`、`the_shutdown_waits_for_an_extraction_in_progress`（`U/updater/tests.rs:281,346`）。runtime DNS guard 的 `a_restore_pending_record_keeps_its_baseline`、`an_empty_baseline_is_still_a_baseline`（`backend/nyanpasu-runtime/crates/nyanpasu-core-manager/tests/dns_override.rs:518,567`）由既有 runtime CI 继续验证，不修改 submodule。
- `rg 'runas|osascript|with administrator privileges' backend/nyanpasu-core/src --glob '*.rs'` 不得有执行站点；注释/测试错误文字逐条列入 PR，不以这些词证明调用存在。`rg 'core_lifecycle::adapters::FsBinaryInstaller|utils::sudo|service_host_adapter::OsServiceHostAdapter' backend/nyanpasu-core/src backend/tauri/src` 退出码 1；`rg 'nyanpasu_(updater|owner_support|net)::' backend/nyanpasu-os/src` 退出码 1。
- 反向检查：丢弃复制任务的 staging lease，使 C2a lease 测试失败；删除 companion 回滚，使 `a_failed_swap_restores_the_previous_files` 失败；引入 core → OS 普通依赖或加回 core → runas，使依赖 gate 失败。bindings 三份零 diff；`cargo check -p nyanpasu-os -p clash-nyanpasu --target x86_64-unknown-linux-gnu`，macOS CI、musl runner。

---

## C2d — 提权同意端口与拒绝实现

本卡是行为变更：拒绝策略不启动任何提权命令；桌面仍由原 OS 授权框取得授权，不加第二个确认框。

- [x] contracts 新增 `elevation` 模块，消费者自有 `ElevationConsent: Send + Sync + 'static`，同步且不做 IO 的 `authorize(ElevationOperation) -> Result<(), ElevationConsentError>`；`ElevationOperation` 用 serde snake_case/Specta 枚举 `Service { command: ServiceCommand }`、`DnsFlush`、`CoreBinaryCopy`（tag `kind`），只标识动作，不接收命令字符串。服务调用先检查策略再准备可能启动进程的 helper；非提权 probe 与失败恢复照常，不把拒绝解释成 service 已停止。`ElevationConsentError` 是模块自有 snafu 枚举，只有 `AuthorizeElevation { operation: ElevationOperation }` 一个失败步骤变体（tag `kind`，wire `authorize_elevation`）；没有应用内确认，不区分 Denied/Unsupported。本轮只是“是否允许请求 OS 授权”的策略，不宣称预先得到 OS 授权；OS 对话框的取消继续按原执行失败分类。
- [x] `O/elevation` 提供 `RefuseElevation`（返回 `AuthorizeElevation { operation }`，保留调用动作）；`T/adapters/elevation.rs` 提供 `InteractiveElevationConsent`（允许现有 OS 机制），`T/setup.rs` 显式注入各 OS 构造器，不用 global。同步策略足够：本轮没有额外 UI 等待，OS 授权阻塞仍在原 blocking task；未来需要异步确认时另评审，不提前抽象。
- [x] C2c 三类机制在启动提权命令前调用策略：服务 install/update/uninstall/start/stop/restart，包含 macOS sudo；Windows/macOS DNS flush；普通复制失败后的 runas fallback。拒绝时 runas/osascript 调用数为零，普通复制成功不请求授权；不改变 actor timeout/outstanding helper、服务 probe/恢复、staging lease、companion best-effort、DNS guard。
- [x] 消费模块新增失败步骤变体：`ServiceCommandError::AuthorizeServiceCommand { command, source: ElevationConsentError }`、`SystemDnsError::AuthorizeDnsFlush { source }`、`InstallCoreBinaryError::AuthorizeCoreCopy { core, destination, source }`；嵌套 source 保留 serde/Specta，不转字符串。同步 core 的分类 match；明确永久拒绝不可变成自动重试授权。现有 updater 的 `Failed(String)` 保持，C2e 在 admission 前拒绝 HTTP update，不扩大异步 updater wire 模型。
- [x] 前端仅在 `ipc-error.ts` 的 DNS/install mapper 加 `authorize_dns_flush`、`authorize_core_copy` 分支和共用 consent mapper；只新增 `error_elevation_authorize` 消息；consent mapper 处理 `authorize_elevation`，不按 operation 增加消息。`ServiceCommandError` 的新变体经现有 `prepare_service_install_prompt` generic message 路径无需新增 exhaustive switch（`ipc-error.ts:448`），服务 CoreFailure 映射保留；不改页面。重新编译 Paraglide，逐包 typecheck。
- [x] Cargo 无新包，contracts/OS 与 shell 的 feature 约束不变；gate/prefix/musl 条目不增。新增测试（新）：`refused_service_command_never_spawns`、`refused_dns_flush_never_spawns`、`refused_copy_fallback_never_spawns`、`successful_plain_copy_does_not_request_elevation`、`interactive_policy_adds_no_confirmation`、`refused_service_command_keeps_the_probed_phase`；通过注入 command runner 断言，不在测试机弹框或改系统状态。同步 design/路线图 C2 consent 状态与 E4 注入责任。

- [x] 修改 `ServiceActorState::command_error`（`C/actor_v2/service_actor.rs:301-306`）对授权拒绝设 retryable=false；普通命令执行失败仍 true。新增 typed-client 测试 `service_consent_refusal_is_not_retryable`，断言返回 CoreError.retryable == false；另断言普通 RunElevated/退出失败保持原 retryable。新增 `consent_refusal_pauses_automatic_service_recovery` 于 `C/client/application_workflow/tests/service_recovery.rs`：用可计数授权 fake 与确定性时钟/显式 tick 驱动 `core_lifecycle/workflow.rs:422-426` 的 pause，后续多个自动 tick 不再调用 authorize，普通执行失败仍按现有恢复策略重试；不使用 sleeps。
- [x] 在 `frontend/nyanpasu/tests/ipc-error.browser.test.ts` 新增 `dns_elevation_refusal_uses_the_elevation_message`：公共 ipcErrorMessage → 私有 systemDnsErrorMessage（ipc-error.ts:327），输入嵌套 authorize_dns_flush/source.authorize_elevation，断言 `m.error_elevation_authorize()`；新增 `core_copy_elevation_refusal_uses_the_elevation_message`：ipcErrorMessage → runtimeErrorMessage 的 install_core_binary（:446-447）→ 私有 installCoreBinaryMessage（:658），嵌套 authorize_core_copy/source.authorize_elevation，同样断言该消息。不要为测试导出私有 mapper。消息写 `frontend/nyanpasu/messages/{en,ja,ko,ru,zh-cn,zh-tw}.json`；重新编译 Paraglide 后运行 `pnpm run test:frontend` 及既定逐包 typecheck。

**验证**

- `cargo test -p nyanpasu-contracts`、`-p nyanpasu-os`、`-p nyanpasu-core`、`-p clash-nyanpasu`；§0.8 清单 = C2c + 新测试。既有 `a_failed_install_leaves_the_probed_phase_not_the_transitional_one`（`C/actor_v2/service_actor.rs:1955`）、`recovery_distinguishes_failed_start_from_an_unconfirmed_timeout`（`:1762`）与 C2c 关闭/lease 测试通过；新增三个消费错误及单一 refusal 的 Display/source 链/serde 断言，验证 operation 在 source 中保留。
- bindings 仅新增 `ElevationOperation` 三变体、`ElevationConsentError.authorize_elevation { operation }` 及上述三个消费错误变体；core、destination 的原 wire 类型/命令签名/UpdaterState 不变。前端新增一个消息、两个消费错误分支和共用 mapper；运行本节三个显式包 typecheck。
- `rg 'nyanpasu_(core|updater|owner_support|net)::' backend/nyanpasu-os/src` 退出码 1；逐一核对全部 runas/osascript 启动点经过授权，不把关键字无匹配当作 OS 内的目标。反向检查：任一站点绕过 authorize，对应 never_spawns 测试失败；把拒绝改成 retryable，phase/retry 断言失败。`cargo check -p nyanpasu-os -p nyanpasu-contracts -p clash-nyanpasu --target x86_64-unknown-linux-gnu`，macOS CI、musl runner。

## C2e — `update_core` 的最小 HTTP 限制

本卡是行为变更：WebUI 的 `update_core` 在 job admission 前返回既有 unsupported / HTTP 501；桌面更新不变。

- [x] 只去掉 `T/ipc.rs:1125` 的 `http` opt-in，不改 `update_core` 的桌面签名和 `download_core_update` 调用（`:1128`）。宏 `backend/nyanpasu-macro/src/unified_command.rs:173` 已生成 unsupported；保留 `T/unified_rpc.rs:174,189` 的 kind/status，不新增 DTO。
- [x] 新增 `http_update_core_is_unsupported_before_job_admission`（新）：通过真实 HTTP registry 入口，断言 501、kind unsupported、updater admission/prepare/install 都未发生；新增 `desktop_update_core_dispatch_remains_available`（新），验证桌面 generated dispatcher 仍注册。不用后续 `UpdaterState::Failed(String)` 冒充原 HTTP 调用的结构化拒绝。
- [x] 在 design §8/§9 与路线图保留 #6157 已知缺口：`patch_app_config` 开服务（`C/client/application_workflow/tcc.rs:867`）、关服务（`:1290`）、`retry_configuration_runtime`（`:299`）和 compensation（`:1383`）仍共享桌面策略。C2 不引入逐调用权限上下文、不另建第二个 client；E4 独立 server 必须注入拒绝策略，并验证上述路径均不能弹框。不能把本卡描述为“所有嵌入式 WebUI 提权路径已关闭”。
- [x] Cargo/gate/静态前缀/musl 不变；无前端 switch/message 新增。

**验证**

- `cargo test -p nyanpasu-macro`、`cargo test -p clash-nyanpasu`；§0.8 清单 = 原清单 + 两新测试；既有宏 `http_requires_opt_in_and_accepts_only_lifetime_generics`（`unified_command.rs:514`）及 `errors_preserve_domain_codes_and_operation_identity`（`T/unified_rpc.rs:1010`）通过。
- `rg -n -B 3 'pub async fn update_core' backend/tauri/src/ipc.rs` 必须显示无 http 的属性；此正向定位不能按退出码 1 判通过。另对该单个函数的 AST/宏展开断言 HTTP handler 是 unsupported。反向检查：恢复 `rpc(http)`，admission 前拒绝测试失败。
- 三份 bindings 零 diff，前端变体/消息零新增；仍运行 rpc/query/nyanpasu 三个显式包 typecheck。只改 transport opt-in，无平台分支检查新增要求。

## C2f — RPC dispatch 的审计切面

本卡是行为变更：应用 API 调用产生无 payload 的结构化 tracing 审计事件，命令结果、执行顺序、取消与 panic 语义不变；不建存储、轮转或 UI。

- [x] 在 RPC 消费层 `T/unified_rpc.rs` 定义 `RpcAuditSink: Send + Sync + 'static { fn record(&self, record: &RpcAuditRecord); }`，无 Result、无 async；`RpcDependencies`（`:22`）显式注入 `Arc<dyn RpcAuditSink>`，HTTP weak routes（`:213-239`）使用同一依赖，不能另建全局 sink。端口/值以后随 registry 移到 `nyanpasu-rpc`（E2），不进 core/contracts，不带 Tauri 类型。
- [x] record 只含 transport（`TauriIpc`、`Http`；E3 加 `LocalSocket`）、command（注册表的固定名称；未知名称归为 `unknown_method`，不记录任意输入文本）、现有 owner identity（`RpcOwner`，桌面 window label / HTTP session 或现有匿名 owner）、单调 duration、outcome（Success、Failure 的 kind/code、Cancelled、Panicked）。不记录 params/output、error message/detail/domain_error、headers、URL、token、SSE 帧；kind/code 只取 `RpcError` 的分类字段。无新 serde/Specta DTO，record 不对外传输。
- [x] 新增唯一公共内核 `dispatch_audited`：在开始 poll 命令前创建记录 guard，把 lookup、宏生成 handler 的解析/调用/结果序列化全包在其内部；完成时同步记录再原样返回同一个 `Result`，不重试、不 spawn、不加 timeout、不改 error。`UnifiedRpc::call`（`:278`，HTTP）和 `call_tauri`（`:286`）都只经此内核；后者从 window label 构造 owner。未知方法、invalid params、unsupported 均记录。guard 的 Drop 用 `std::thread::panicking()` 区分 Panicked 与 Cancelled，记录后让原命令 panic 及 payload 继续传播。sink 是本项目自有实现，契约要求不得 panic、不得向调用者返回错误；遵守 design §7.3，不新增 `catch_unwind`（包括测试），不提供 sink panic 恢复分支；panic=abort 不承诺 Drop 记录。同步记录只增加自身执行开销，不引入任务/锁等待式队列。
- [x] 真实入口：`T/lib.rs:408` 只注册 `T/specta_export.rs:15` 的 `call_rpc` transport builder，`T/unified_rpc.rs:383-393` → `call_tauri`；HTTP `:452-470` → `call`。宏 `unified_command.rs:164-173,176` 生成两类 handler，inventory 项只由 registry 调用，**不在宏 handler 再记一次**。保留并扩展宏覆盖测试 `every_ipc_rpc_command_has_a_desktop_dispatcher`（`:552`）、`every_ipc_tauri_command_uses_unified_rpc`（`:587`），新增 transport builder 只注册 `call_rpc` 的结构测试，防止注册原始 command wrapper 绕过切面。
- [x] 当前不是所有 HTTP API 都经过 command table：`http_logs_archive:302`、`http_connection_details:343`、`http_events:401` 直接调用应用。它们也调用同一个泛型 `dispatch_audited` 内核（可返回 Value、Response 或订阅响应，不能为审计把 stream 收集为 Value），分别用固定操作名 `logs_archive`、`connection_details_subscribe`、`events_subscribe` 和相同 HTTP owner 解析（给这三个 handler 增加 HeaderMap 提取，复用 `http_call:465-466` 的 session/匿名 fallback，不另加认证要求）；记录响应/订阅建立的结果，不逐帧记录、不延长 stream 生命周期。把 `http_call` 的 JsonRejection、events 的 QueryRejection 转为经过内核的 invalid_params 记录（无可用 command 时用固定 `invalid_envelope`），保留原 HTTP status/body。静态资源、鉴权中间件拒绝和 Tauri 在进入 `call_rpc` 前拒绝的畸形 transport envelope 不是已分派应用调用，不宣称此层能看到它们；将来若要网络安全接入日志另立卡，不在原始 payload 层补日志。
- [x] `T/adapters/rpc_audit.rs` 在同一 target `nyanpasu::rpc_audit` 按 transport 选结构化事件级别：HTTP 为 INFO，Tauri IPC 为 DEBUG；E3 local socket/CLI 为 INFO。GUI 是受信任的本地前端且高频轮询，DEBUG 避免默认日志被查询淹没；WebUI/CLI 是主要审计入口。级别不随 success/failure/cancel/panic 改变。`T/setup.rs:586-596` 显式注入，测试用内存 sink。E3 local-socket/CLI 必须进入同一内核，不能在 transport 自己复制一套审计流程。
- [x] 新增测试（新）：`tauri_dispatch_records_one_audit_event`、`http_dispatch_records_one_audit_event`、`direct_http_routes_use_the_same_audit_seam`、`audit_records_rejected_dispatches`、`audit_never_records_payloads_or_error_details`（在请求、响应、错误文本放唯一 secret，并断言 record 和 tracing 输出都没有）、`audited_failure_preserves_the_original_rpc_error`（包括 code/retryable/operation_id/domain_error）、`audit_level_follows_transport_not_outcome`（在 tracing 输出断言 HTTP INFO、Tauri IPC DEBUG，成功/失败同级，E3 增加 local socket INFO）、`dropping_audited_call_preserves_cancellation`、`audited_command_panic_keeps_its_payload`。Tauri 测试必须走真实 `call_tauri`/generated handler 接线，不能只测 helper；HTTP 用 router 请求；同步 sink 以调用序列断言不重排；命令 panic 测试经 `tokio::spawn` 的 JoinError 验证原 payload 及 Panicked 记录，不加 `catch_unwind`。
- [x] Cargo 无新依赖/包，gate/prefix/musl 注册不变；三份 bindings、前端 switches/messages 均零 diff。同步 RPC 路线图的 E2 迁移/E3 接入要求。

- [x] 测试 sink 同提交注入全部 RpcDependencies literals：`T/unified_rpc.rs:504,882,968,1094,1214`；生产 `T/setup.rs:586-596` 同步。不能漏掉 HTTP lifecycle/owner 隔离 fixtures。

**验证**

- `cargo test -p nyanpasu-macro`、`cargo test -p clash-nyanpasu`；§0.8 清单 = 原清单 + 新测试；`errors_preserve_domain_codes_and_operation_identity`（`T/unified_rpc.rs:1010`）、`connection_details_send_current_and_new_frames_and_release_demand`（`:1036`）、`enabled_http_server_joins_application_shutdown`（`:1074`）通过。
- `rg -n 'handler\.[01]|http_handler|tauri_handler|dispatch_audited' backend/tauri/src/unified_rpc.rs backend/nyanpasu-macro/src/unified_command.rs` 用于枚举所有分派点，逐项证明只经内核，不把正向枚举当无匹配检查；`rg 'params|payload|domain_error|error\.message|headers' backend/tauri/src/adapters/rpc_audit.rs` 退出码 1。运行本节三个显式包 typecheck，bindings 零 diff。
- 反向检查：任一 transport 直接调用 handler、直接 HTTP route 跳过内核、宏 wrapper 重新注册到 invoke handler，覆盖/一次记录测试失败；向 event 加 error detail，secret 测试失败；把 command error 替换成日志错误，原错误保留测试失败；把等待改 spawn 后 join，取消/顺序测试失败；把 IPC 改为 INFO 或失败升为 ERROR，transport 级别测试失败。

---

## C3a — 桌面交互机制移到 tauri shell

本卡是纯移动，依赖 C1a 的消费接缝；不依赖 C2。建议 C1b 后合入以减少 setup/fixture 冲突。tauri 已依赖 core，desktop 可直接使用 core 公开的解析、plan/status 与消费契约，不新增 crate 或反向依赖。

- [x] 新建 `D/hotkey/`，接收 `C/client/hotkey/{actor,error}.rs`、`mod.rs:45-133` 的 `HotkeyArgs`、spawn `Error`、`HotkeyStatus`、`HotkeyClient` 与 stopped health；`ports.rs:139,326,343` 的 `ShortcutError`、`ShortcutRegistrar`、`HotkeyActionSink` 随机制走。owner 对已注册状态计算并执行 diff 的逻辑（`actor.rs:161-220`）随 actor 移动；纯 `HotkeyBindings::diff`/`HotkeyOp` 仍在 core。`D/main_thread.rs` 接收 `C/client/main_thread.rs` 全部；`D/effects/` 接收 C1a 的 `PresentationEffectExecutor`、`LocaleSink`、`TrayRefresher/TrayError`、`WidgetController/WidgetError` 与 full-refresh pending。具体 GUI adapters 仍在 `T/adapters/`，由 `T/setup.rs` 组装。
- [x] core 保留 `PresentationEffectsPort`、`AcceleratorValidator`、`HotkeyBindings`（parse/shape/canonical/duplicate/diff）、`HotkeyOp`、`HotkeyParseError`、`HotkeyAction`、`WindowControl/WindowError`；desktop 直接 import，不再包装列表验证器。`hotkey::validate_bindings`（`C/client/hotkey/mod.rs:140`）与 `NyanpasuClient::dispatch_hotkey_action`（`:154`）留 core；共享 executor、`CoreLogCaptureEffects`、`SystemProxyClient`、`LoggerRefresher/LoggerError/LogRotation`、plan/actor/status/receipt 不动。`WidgetError` 的 widget IPC source（`C/client/effects/ports.rs:117,126`）随机制走，不进 contracts。
- [x] 跨 crate 可见性按实际调用开放：`C/lib.rs:33` → `client/mod.rs:35,41` → `hotkey/mod.rs:11`、`effects/mod.rs:8-10` 已是公开模块链；hotkey 的上述值/trait（`ports.rs:35,106,161,170,190,315,351`）、parse/accelerators/diff/From（`:199,249,256,282`）、`has_super_key`（`:304`）已 public。plan 的 `EffectKind/TrayRefresh/WidgetLogoState/WidgetLogoDesired/TrayView/ApplicationEffect/ApplicationEffectPlan`（`plan.rs:200,215,245,253,287,339,373`）及其字段、`kind/from_effects/effects`（`:354,378,401`），status 的 `EffectRevision` 与 new/get、`EffectFailureCode/EffectHealth/EffectStatus/failure_text`（`status.rs:15-24,36,89,106,74`）也已 public；不公开 plan 私有 fields/投影内部结构。C1a 的 `effects::status::{healthy,degraded}` 从 `pub(crate)` 改为 `pub`，供两 crate 共用；`C/client/test_support.rs:870` 的 `AnyAccelerator` 从 `pub(crate)` 改为 `pub`，仍由 `client/mod.rs:59-60` 的 test-support gate 约束，供迁入的 actor 测试复用。除此之外不扩大 core 可见性；`CoreLogCaptureEffects`、`validate_bindings`、WindowError 的 snafu selectors 保持原可见性。HotkeyParseError 的 selectors 已 public（`ports.rs:104`）；HotkeyEffectError 的 selectors 随模块进 tauri 后仍 `pub(crate)`。
- [x] 调用方同提交迁完：`T/setup.rs:205,676-727`、`T/adapters/{hotkey,main_thread}.rs`、`T/adapters/ui_effects/`、`T/utils/exit.rs:204`、`T/core/tray/mod.rs:687`；core 的 `client/app_lifecycle.rs:113,345` 等直接组装 hotkey owner 的集成夹具改用消费端口 fake，真实热键释放由下列迁入测试及 tauri shutdown 测试覆盖，不能让 core dev-depend tauri。删除 core main_thread、hotkey actor/error 的模块声明与旧 re-export（`C/client/mod.rs:104`），但保留 hotkey 的 core 域模块。
- [x] hotkey 原 18 例中 **9 例移到 `D/hotkey/tests.rs`**（以下锚点均为 `C/client/hotkey/tests.rs`）：`update_unregisters_before_registering:274`、`unchanged_bindings_produce_no_os_calls:311`、`partial_registration_failure_degrades_and_keeps_successes:331`、`a_refused_grab_is_retried_by_the_next_reconcile:370`、`reconcile_validates_every_binding_before_releasing_any:394`、`stale_revision_is_superseded:437`、`exit_unregisters_all:465`、`reconcile_after_the_cancel_is_rejected:485`、`callback_dispatches_action_to_sink:525`。`RecordingRegistrar` 与 owner fixtures 随测试走，复用公开的 `AnyAccelerator`，不复制它。
- [x] **9 例留 core**：`parse_accepts_the_legacy_func_comma_key_format:27`、`parse_rejects_malformed_unknown_invalid_and_missing_super:49`、`parse_rejects_duplicate_accelerator:77`、`diff_emits_unbind_rebind_and_bind:96`，以及 facade 的 `dispatch_toggle_system_proxy_flips_app_config:603`、`dispatch_toggle_tun_mode_flips_clash_config:638`、`dispatch_clash_mode_patches_runtime_overrides:670`、`dispatch_dashboard_goes_to_the_injected_window_control:690`、`invalid_hotkey_is_rejected_before_commit:709`。`HotkeyBindings::from_pairs/as_map`（`ports.rs:232-243`）只被留下的测试用，保留 `cfg(test)`，不为桌面公开它们。
- [x] main-thread 四例全移：`run_returns_the_task_result:152`、`a_task_stranded_in_a_lost_event_loop_stops_its_wait:178`、`run_fails_when_the_event_loop_drops_the_task:199`、`work_goes_to_the_taker_only_once_it_has_taken_over:214`（原 `C/client/main_thread.rs`）。`T/adapters/ui_effects/tests.rs` 的纯展示执行测试随 `D/effects/`，依赖真实 Tauri/widget adapter 的测试原地；shared owner 测试仍在 `C/client/effects/tests.rs`。不复制测试/fixture。
- [x] 不建 crate，workspace/IMPLEMENTATION/CORE_FAMILY/STATIC_GATE_PREFIXES/musl 包列表均不加 desktop；tauri → core/config 的 normal/dev 边仍关默认 feature，tauri dev 边启用 core `test-support` 以取得 `AnyAccelerator`。删除 core `nyanpasu-widget-protocol` 直接依赖（`backend/nyanpasu-core/Cargo.toml:65-67`）；tauri 按实际用途保留/声明它。core 本来没有直接 global-hotkey/keyboard-types 依赖，不声称移除了这些库。
- [x] 保留 `S/backend-boundaries.ts` 的现有依赖规则；不新增源级或直接依赖 manifest 门禁，也不加整个 widget-protocol 的传递禁令：`backend/nyanpasu-config/Cargo.toml:22`、`src/application/widget.rs:1` 仍消费纯值 `StatisticWidgetVariant`，协议的 `ipc` 是可选 feature（`backend/nyanpasu-widget-protocol/Cargo.toml:12`）。`backend-boundaries_test.ts` 保留/补齐 core → config → render-free widget-protocol 正例及 core → tauri/widget GUI 负例；tauri 已在静态前缀内，`S/architecture-ledger_test.ts` 补 D/ static 覆盖。下列机制/manifest 搜索仅作 PR-time 检查，不加入永久扫描规则。更新 backend-packages/architecture/路线图 C3 完成状态；依据 A、B 测量无桌面编译收益、server 不需这些机制，不新增交互 crate。

- [x] C1a 新增 `failed_full_refresh_is_retried_until_success_then_part_stays_part` 随展示 executor 移入 D/effects/；测试清单在原 21 例基础上计入该例，late-full 乱序测试独立保留。duplicate-list 提交前测试留 T/adapters/hotkey/tests.rs。
- [x] 随主机实现移动的测试 guards 改为 tauri 本地 `cfg(test)`/`cfg_attr(test, mockall::automock)`：`C/client/main_thread.rs:116,119` 的 InlineMainThread 及 impl，`effects/ports.rs:25,66,148` 的 locale/tray/widget mocks，`hotkey/ports.rs:324,342` 的 registrar/callback mocks。tauri 不新增 test-support feature；依赖 core 的 test-support 只供仍在 core 的公开 fixtures/ports（如 AnyAccelerator/WindowControl），不能把依赖 feature 当作 tauri 自己的 feature。

**验证**

- `cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；§0.8 两侧测试名清单合计不变，仅文档化的模块前缀变化，逐项核对上述 9 移/9 留与 main-thread 四例。core `commit_receipt_and_status_keep_source_separate_from_pending_notifications`（`effects/tests.rs:803`）、tauri `locale_is_applied_before_tray_refresh:236`、`late_full_tray_refresh_still_runs_after_a_newer_part_refresh:663`、`the_owners_clean_up_independently_and_the_shutdown_waits_for_all:1171`（`adapters/ui_effects/tests.rs`）通过。
- PR-time：`rg 'struct HotkeyClient|struct HotkeyActor|struct MainThreadHandoff|trait MainThreadExecutor|struct PresentationEffectExecutor|tray_full_pending|nyanpasu_widget_protocol::' backend/nyanpasu-core/src backend/nyanpasu-migration/src backend/nyanpasu-owner-support/src backend/nyanpasu-observers/src backend/nyanpasu-updater/src` 退出码 1；`rg 'nyanpasu_core::client::main_thread|nyanpasu_core::client::MainThread|client::hotkey::HotkeyClient' backend/tauri/src` 退出码 1，另审查分组导入；`rg 'nyanpasu-widget-protocol' backend/nyanpasu-core/Cargo.toml` 退出码 1。正向核对 `HotkeyBindings::parse`/`diff` 与 `validate_bindings` 仍在 core；不把这些调用误判成桌面机制。
- 反向检查：core 加回 widget-protocol 直接依赖或移回 hotkey owner/handoff，以上 PR-time 无匹配检查失败，不声称永久 gate 拦截；core 引入 tauri 依赖使现有依赖 gate 失败；丢失 event-loop-loss 信号使 main-thread 测试失败；full pending 变局部变量使新增 `failed_full_refresh_is_retried_until_success_then_part_stays_part` 失败。bindings 三份零 diff；`pnpm run test:scripts`、`pnpm run test:architecture-ledger`、`pnpm run test:backend-musl`（只跑原中立包，desktop 测试由 tauri CI 跑）；`cargo check -p clash-nyanpasu --target x86_64-unknown-linux-gnu`，macOS CI。

---

## Phase B–E 里程碑卡

每张卡开工前先细化为与 Phase A 同粒度的任务并审阅；B1a–B4 已细化为 §2 的卡，C1–C3 已细化为 §3 的卡。依赖图、决策、前置、不变量、完成判据见 design §5。

| 卡  | 内容                                                                                                             | 前置                      | 完成判据（摘要）                                                               |
| --- | ---------------------------------------------------------------------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------------ |
| A2b | 脚本扩展注入与引擎 feature                                                                                       | A2                        | design §5.0                                                                    |
| B1a | persistence 收 format 与通用原子写；`session_state` 不依赖 migration                                             | A5b                       | 无 `session_state → migration`；两个恢复测试通过                               |
| B1b | backup/migration 的存储端口；migration crate                                                                     | B1a                       | migration crate 不依赖 redb                                                    |
| B2  | facade 签名去具体存储类型；traffic legacy 导入器 feature 化                                                      | B1b                       | `ClientSetupArgs` 类型闭包无 `Storage`/redb；traffic 无默认 redb               |
| B3  | logs、traffic、geo 成 owner crate；`owner_shutdown` 按需建 crate                                                 | B2                        | owner 生产代码不含 clash/core-control/redb/notify；traffic 回归测试通过        |
| B4  | updater crate；后端与其 HTTP 客户端由 shell 注入                                                                 | B3a                       | facade 不构造 `HttpUpdaterBackend`；facade 无归档依赖                          |
| C1  | 展示能力可选、结构化不支持（已细化为 §3 C1a、C1b）                                                               | A                         | 无展示能力的装配测试；显式热键编辑在提交前被拒                                 |
| C2  | `nyanpasu-os` 与提权同意端口；能力端口进 contracts；`update_core` 去掉 HTTP；RPC 审计切面（已细化为 §3 C2a–C2f） | A、B4                     | core 无提权执行点；拒绝策略不启动提权命令；经 HTTP 的 `update_core` 得到不支持 |
| C3  | 桌面交互机制移入 tauri 外壳；共享收敛与展示效果分离（已细化为 §3 C3a）                                           | C1                        | core 族不含桌面机制、不直接依赖 widget-protocol；效果与热键回归测试通过        |
| D1  | 状态引擎（约 7,000 行完整闭包）                                                                                  | B1a                       | 引擎测试名清单不变且通过；不依赖 core 族其他成员                               |
| D2  | 反转 mutation 参与；运行时值下移；拆 core-control 与应用                                                         | D1、B3、B4、C1b、C2f、C3a | 无具体 workflow client；workflow 取消/恢复/关闭测试通过                        |
| E1a | 宏中立声明、按目标 opt-in 与能力元数据；protocol 信封                                                            | A                         | protocol 无 core 族/Tauri；bindings 无 diff                                    |
| E1b | 完整请求/响应 DTO 进 protocol                                                                                    | E1a、D2                   | protocol 类型闭包不经 core 族                                                  |
| E2  | `nyanpasu-rpc` 注册表与事件总线                                                                                  | E1a、C2f                  | 注册表与审计端口无 Tauri 类型                                                  |
| E3  | 纯 API WebUI；local socket 传输                                                                                  | E2                        | 无资源启动；未 opt-in 被拒；跨传输一致                                         |
| E4  | `nyanpasu-server`，然后 `nyanpasu-cli`                                                                           | C2d、C3a、E1b、E3         | CLI 不链接 core 族；同一 server 会话；拒绝策略关闭 #6157 剩余路径              |
| E5  | OpenWrt                                                                                                          | 用户明确恢复              | 单独验收                                                                       |

B 结束时按 design §6 复测编译时间。

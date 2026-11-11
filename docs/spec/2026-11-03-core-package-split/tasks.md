# Core 包族拆分、Shell 与 RPC 目标 任务

关联设计：`design.md`（同目录）。路径缩写同 design §1。

## 0. 全局约束

1. 遵守 `AGENTS.md` 与 `docs/development/*`：不加全局单例；依赖显式注入；不给 core 族加 Tauri 依赖；`NyanpasuClient` 只是 facade。
2. 移动与保行为接缝只移动代码、注入依赖、收窄依赖方向。行为改动单独成卡（A2b、D2c 的严格结算时序、C1 的不支持结果、C2 的同意策略、HTTP 限制与 RPC 审计，E 的能力 admission/API-only/local/server/CLI），不夹在移动提交里；同一 PR 内的卡也分别提交。行为卡的前端例外仅限 design §9；移动中发现的现存问题记在 PR 描述里，不顺手改。
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

## 4. A2b 与 Phase D 的 PR 划分

本节细化 design §5.0/§5.3，源码行号记录于 `fa7e5050b`（A、B、C 已完成），按符号定位；各实施卡已交付，标为「新」的测试已随卡落地。偏差见 design §8.1 V-23–V-29，决策见 §8.2 O-10–O-17，全局约束 §0 适用。PR-D0 是历史文档细化；每张卡一个可独立编译、测试通过的提交，同一 PR 按箭头顺序实施，不把行为修正折进移动提交。

- 缩写：`W/` = `C/client/application_workflow/`，`J/` = `backend/nyanpasu-scripts/src/`，`B/` = `backend/boa_utils/src/`；已交付包 `E/` = `backend/nyanpasu-state-engine/src/`、`V/` = `backend/nyanpasu-runtime-values/src/`，此处 `E/` 不是 Phase E。工作名按 O-4，建包时确认。父 workspace 指 `backend/Cargo.toml` 的成员，不把独立 runtime workspace 的中立工具当父 workspace 成员。
- 实施栈固定为 D1 → D2 → D3 → D4 → A2b。A2b 的代码前置仍只有 A2；排在 D4 后是合入顺序，避免它的新 pipeline error 与 D4 的错误闭包迁移同时改两份定义。A2b-b 只改最终值 owner 引用的 config pipeline 错误，不复制 `RuntimeError`/`RuntimeBuildError`；PR-D0 当时不写 E 卡；后续 E 细化见 §5，E 与 POC 决策仍由 design O-13 约束。
- Rust 命令从 `backend/` 执行，`pnpm run` 从根目录执行。所有卡执行 §0.7；测试清单按 §0.8 保存、检查退出码/非空、排序比较，新增测试单列。验证块保留各卡验收命令；复选框记录代码交付，未完成的执行检查单列，不表示本次文档同步重跑了历史验证。源路径 `rg` 使用显式文件或 `--glob '*.rs'`，不用 shell 花括号；1 = 无匹配，2 = 检查失败；分组 `state::{...}`、别名和相对导入须另审阅，单条正则不能证明闭包已迁完。
- 新包同提交更新 workspace/消费者 manifest、`S/backend-boundaries.ts:27-124` 和 `S/backend-boundaries_test.ts`、`S/policy.ts:81-95` 的 `STATIC_GATE_PREFIXES`、`S/architecture-ledger_test.ts:996-1010` 的覆盖矩阵，以及 `scripts/src/testing/test-backend-musl.ts:68-69,77-90` 的 `-p` 与 `expectedTargets`。新增集成测试目标必须登记；下列新 Rust 测试默认放 unit-test 模块，不暗增集成目标。无新包的卡明确保留这些注册。
- 移动/保行为接缝运行 `cargo test -p clash-nyanpasu export_typescript_bindings`（`T/specta_export.rs:448,452,456,486`），三份 bindings 无 wire diff；D2d 的 rpc-bindings.ts 仅有一行 CoreError JSDoc 链接改纯文本（design V-66），其余移动零 diff；不手改生成文件。A2b-b 只允许其验证块列出的 wire 变化。新模块错误用 snafu、按失败步骤命名；保留原错误对象、Display/source 链与 serde skip，不用字符串替代来源。原 engine 的 thiserror/anyhow 不顺手迁框架。
- D1 无需 core 测试夹具。D2a 的 impact 测试含 macOS 分支（`W/impact.rs:934`），保留并交 macOS CI；D2e 只移定义，不移 `C/client/runtime.rs:427,559,609,621` 的 Unix IO 或 `C/actor_v2/local_host.rs:69-74` 的 macOS 装配。触及平台分支时按 §3 的 Linux 检查/macOS CI 规则，不把跨编译当平台行为测试。
- 每卡完成后同步本卡/里程碑与路线图；建包或模块策略卡同步 backend-packages/architecture 的目标与已完成状态，未实施不标完成。engine 为泛型依赖约束，runtime-values 为 E1b 无 core 族 DTO 闭包，不为新包声称桌面编译收益。

| PR     | 卡              | 依赖                | 说明                                                                                     |
| ------ | --------------- | ------------------- | ---------------------------------------------------------------------------------------- |
| PR-D0  | 本次文档细化    | A、B、C             | 记录决策与卡，不实施代码                                                                 |
| PR-D1  | D1a             | B1a                 | 完整泛型 engine 与全部测试同迁                                                           |
| PR-D2  | D2a → D2b       | D1a、B、C           | core 内源契约/receipts 归位，然后反转工厂；两个提交                                      |
| PR-D3  | D2c             | D2b                 | 独立 test-first 行为修正：`RecoveryRequired` 等待源权威决定                              |
| PR-D4  | D2d → D2e → D2f | D2c                 | 错误接缝、公开值纯移动、模块策略分别提交；不建 control/workflow crate                    |
| PR-A2b | A2b-a → A2b-b   | A2；合入在 PR-D4 后 | 扩展保行为接缝；feature/typed error/pipeline/wire/前端在一个行为提交内完成；代码不依赖 D |

---

## D1a — 完整泛型状态引擎

本卡是纯移动；不改投票、持久化、CAS、回滚、drop、panic 或结算行为。

- [x] 新建 `nyanpasu-state-engine`，同迁 `C/state/manager.rs`、`manager/{persistent_builder,persistent_state,simple,weak_persistent_state}.rs`、`transaction.rs`、`transaction/notify.rs`（七文件 3,247 行），以及 `ack.rs`、`builder.rs`、`coordinator.rs`、`decision.rs`、`error.rs`、`snapshot.rs`、`version.rs` 和 `tests/` 全部；完整闭包约 7,000 行。`C/state/mod.rs:1-47` 的泛型声明/`VersionedState` 进 `E/lib.rs`，`:49-55` 的 application/clash/`config_error`/mutation/profiles/`session_state` 注册留 core。manager 是泛型机制，不把 `C/state/mutation.rs` 混入 engine。
- [x] `StateAckSubscriber`/`StateParticipant`（`ack.rs:143,202`）、`DecisionHandle`/私有 writer（`decision.rs:33,58`）、`StateCoordinator`/builder（`coordinator.rs:60,443`）、snapshot/version、各 manager/setup、`ReplaceIfVersionResult`/Error（`manager/persistent_state.rs:136,142`）与 transaction 一起移。修复 `manager.rs:7,13,21` 的 `$crate::state` 与文档链接；保留 `StateStore`/`DecisionWriter`/transaction 的私有边界，不为移动公开内部夹具或建 test-support feature。
- [x] 调用方同提交迁完：`C/client/{application,clash_config,profiles,session_state,traffic}.rs`（泛型导入分别在 `:4,4,11,3,6`）、`client/core_lifecycle/workflow.rs:3`、`client/runtime_error.rs:9`；`W/{attempt,mod,mutation,participant,preparation,tcc,workflow}.rs`（`:17,22,16,13,3,19,3`）；`C/state/{application,clash_config,session_state,config_error,mutation}.rs`、`state/profiles/actor.rs:6,1324-1325`。测试消费者包括 `C/client/mod.rs:1728,1743-1754,1765,3752`、`client/profiles.rs:614,1884`、`W/tests/{closing,mod,mutations,recovery,startup}.rs`（`:10,35,23,151,24`）、`T/ipc.rs:2265,2471`、`T/startup_failure_tests.rs:142`；无旧 re-export。
- [x] workspace 加 member；core 普通依赖 engine，tauri 因直接错误断言加 engine dev-dependency，均关默认 feature。engine 父 workspace 依赖仅 `nyanpasu-persistence = { path = "../nyanpasu-persistence", default-features = false }`；保留 anyhow、bon、arc-swap、async-trait、indexmap、thiserror、tokio、tracing、serde、atomicwrites、camino、fs-err 的现有版本/feature（core Cargo `:91-104`；workspace Tokio/full、fs-err/tokio 在 `backend/Cargo.toml:97,100`）。测试用 tempfile、`serde_yaml_ng`、Tokio test-util（core Cargo `:142-143`）。不为最小 feature 优化改写依赖；core 中仅删除全 crate 已无使用的直接依赖。
- [x] `S/backend-boundaries.ts` 将 engine 加 `CORE_FAMILY`，另禁所有其他 `CORE_FAMILY`；对父 workspace 成员采用仅自身/persistence 的允许集合，按 resolved normal/build 闭包拒绝其余成员，不误禁 persistence 带入的独立 runtime 中立工具。`backend-boundaries_test.ts` 正例 engine → persistence、core → engine；负例 engine → 各 core 族/config/contracts/transform/实现/GUI、foundation/实现 → engine，直接与传递路径都测。不靠「没有 import」代替依赖 gate。
- [x] `S/policy.ts` 加 `backend/nyanpasu-state-engine/src/`，`S/architecture-ledger_test.ts` 加同前缀覆盖；没有新增服务 static，`DecisionHandle` 的实例 `OnceLock` 不作全局允许项。musl 同加 `-p nyanpasu-state-engine`、`lib:nyanpasu_state_engine`。同步两份 development 指南和路线图的 engine 状态/叶依赖规则。
- [x] 内部夹具随迁：`ack.rs:277`、`tests/support.rs:16,30,34,49,53,60`、`coordinator.rs:127`、`manager/persistent_state.rs:455`、`manager/simple.rs:57`；不依赖 core 测试支持。三种 persistent manager 的 blocking panic 原 payload 传播（`persistent_state` `:270-275`、`persistent_builder` `:204-209`、`weak_persistent_state` `:187-194`）原样，不加 owner-support 边。

**验证**

- `cargo test -p nyanpasu-state-engine --lib`、`cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；移动前后按 §0.8 比较下列 102 例，唯一允许的名称变化为 engine 内去掉 `state::` 根前缀。source/workflow/tauri 留下的测试不漏跑；fixture hash 差异仅允许导入/模块路径修正，逐条说明。
- 清单如下，路径相对 `C/state/`，数字为 fa7e5050b 的函数定义行，均为既有测试。inline 测试、test-only helper 与 `tests/` 同迁。

```text
ack.rs
  289: advisory_rejected_counts_as_failure

coordinator.rs
  678: test_new_coordinator
  689: test_initial_version_advances_next_change_id
  711: test_upsert_state_success
  735: test_upsert_with_builder_success
  754: test_upsert_builder_validation_failure
  769: test_required_ack_failure_prevents_commit
  796: test_prepare_failure_does_not_consume_change_id
  829: test_sequential_prepare_stops_after_required_failure
  906: test_advisory_ack_failure_is_ok
  938: test_degraded_ack
  958: test_multiple_subscribers_success
  984: test_state_update_sequence
  1012: test_concurrent_upsert_state_uses_latest_committed_version
  1112: test_blocked_required_prepare_is_waited_for
  1163: test_prepare_ack_is_precommit
  1174: test_with_pending_state_effect_success
  1204: test_with_pending_state_effect_failure_no_commit
  1238: test_with_pending_state_effect_failure_does_not_consume_change_id
  1272: test_with_pending_state_commit_cas_mismatch_returns_error
  1325: test_empty_subscribers_list
  1338: test_snapshot_handle
  1352: test_builder_initialized
  1376: test_error_display
  1389: test_sync_builder_to_async_conversion
  1403: test_advisory_failure_report_helper
  1436: test_remove_subscriber
  1467: test_builder_duplicate_subscriber_replaces
  1497: test_add_subscriber
  1523: test_get_state

decision.rs
  89: the_first_complete_outcome_wins_and_wakes_all_readers
  111: abort_publishes_resource_evidence_atomically

manager/persistent_builder.rs
  335: test_setup_load_success
  356: test_setup_load_file_not_exist
  373: test_setup_load_or_default_success
  390: test_setup_load_or_default_file_not_exist
  408: test_setup_from_builder
  427: test_from_builder_ack_failure_returns_recoverable_manager
  455: test_force_build_returns_manager_after_ack_failure
  477: test_upsert_success
  509: test_upsert_builder_validation_error
  533: test_precommit_failure_does_not_update_current_builder
  595: test_upsert_write_config_error_without_previous
  617: test_upsert_write_config_error_no_commit
  654: test_multiple_upserts
  682: test_config_prefix_in_saved_file

manager/persistent_state.rs
  525: test_setup_load_success
  542: test_setup_load_file_not_exist
  559: test_setup_load_or_default_success
  576: test_setup_load_or_default_file_not_exist
  594: test_setup_from_state
  611: test_from_state_ack_failure_returns_recoverable_manager
  638: test_force_build_returns_manager_after_ack_failure
  659: test_upsert_success
  687: test_upsert_write_config_error_without_previous
  708: test_upsert_write_config_error_no_commit
  739: test_multiple_upserts
  770: test_replace_if_version_success_advances_snapshot_version
  798: test_replace_if_version_with_local_write_owns_the_write_and_its_recovery
  878: test_replace_if_version_stale_version_returns_conflict
  910: test_replace_if_version_persistence_failure_keeps_state_and_version
  938: test_replace_if_version_versions_are_monotonic
  971: test_config_prefix_in_saved_file

manager/simple.rs
  105: test_initialize_ack_failure_returns_recoverable_manager
  128: test_force_build_returns_manager_after_ack_failure

manager/weak_persistent_state.rs
  301: test_setup_load_or_default_existing_file
  318: test_setup_load_or_default_missing_file
  337: test_setup_from_state
  354: test_upsert_success
  382: test_upsert_unreachable_path_still_commits
  404: test_subscriber_ack_failure
  435: test_precommit_failure_does_not_persist_rejected_state
  472: test_force_build_returns_manager_after_ack_failure
  497: test_write_failure_after_successful_commit_preserves_state

tests/cascading_state_migration.rs
  148: cascade_commit_a_to_b_to_c
  168: cascade_no_commit_on_effect_failure
  198: cascade_effect_failure_with_prev_state

tests/mvcc_snapshot.rs
  73: test_snapshot_reflects_committed_state
  81: test_snapshot_updates_on_each_commit
  93: test_snapshot_updated_even_on_post_commit_failure
  106: test_snapshot_not_updated_on_effect_failure
  127: test_snapshot_updated_on_effect_success
  145: test_multiple_handles_see_same_snapshot
  183: test_concurrent_fan_in_no_deadlock
  235: test_concurrent_fan_in_eventual_convergence
  313: test_three_source_concurrent_fan_in
  388: test_snapshot_during_subscriber_reflects_committed_value
  426: test_simple_manager_snapshot_handle

tests/persistence_settlement.rs
  15: upsert_reports_a_lost_commit_without_writing_the_config_back

tests/single_shot_participant.rs
  114: two_failed_attempts_keep_separate_participant_identities
  220: a_committed_decision_is_readable_before_the_commit_notification_lands
  277: the_local_write_runs_between_prepare_and_the_commit
  358: a_failed_local_write_rolls_the_transaction_back
  419: dropping_a_local_write_publishes_an_abort_that_needs_recovery
  460: dropping_before_the_local_write_starts_leaves_the_flag_clear
  529: a_lost_commit_after_a_completed_local_write_is_not_a_clean_rejection
  586: a_version_conflict_settles_the_participant_decision
  630: yaml_failure_waits_for_local_recovery_and_publishes_its_result

transaction.rs
  581: prepared_transaction_drop_publishes_the_abort
  604: pending_prepare_future_drop_publishes_the_abort

version.rs
  77: version_next_panics_on_overflow
  83: state_change_id_next_panics_on_overflow

```

- `rg 'crate::state|nyanpasu_core::state|ApplicationWorkflowClient|MutationReceipt|RuntimeApplyReceipt|nyanpasu_owner_support|ractor' backend/nyanpasu-state-engine/src --glob '*.rs'` 退出码 1；`rg 'state::(Ack|DecisionHandle|StateSnapshot|StateCoordinator|PersistentStateManager|ReplaceIfVersion)' backend/nyanpasu-core/src backend/tauri/src --glob '*.rs'` 退出码 1，另审阅分组 `state::{...}`；不禁止仍合法的 state::profiles 等领域路径。bindings 三份零 diff。
- 反向检查：给 engine 加 core 或 owner-support 依赖，gate 失败；去掉新静态前缀，覆盖测试失败；提前通知但不发布 decision，使 `a_committed_decision_is_readable_before_the_commit_notification_lands` 失败；去掉 abort Drop，使两个 transaction drop 测试失败。保留 `upsert_reports_a_lost_commit_without_writing_the_config_back` 的磁盘证据断言，不加补偿性回写。
- `pnpm run test:scripts`、两门禁、`pnpm run test:backend-musl`；musl 必须发现并运行新 lib 目标，不只成功构建原清单。

**待执行验证**

- [ ] `pnpm run test:backend-musl` 发现并运行 engine 新 lib 目标。（未完成：本地 Docker daemon 不可用，待 Docker 环境执行）

---

## D2a — 源参与契约与内部 receipts 归位

本卡是纯移动（core 内部），不改参与策略、receipt 内容、公开 wire 或 actor 生命周期。

- [x] `C/state/mutation.rs` 变为 `C/state/mutation/mod.rs`，创建 impact.rs、intent.rs、value.rs、`runtime_apply`.rs。`W/impact.rs` 整体移到 source 的 impact.rs，包括 `RuntimeImpact`、`ActivationIntent`、`ContentDigest`、`StagedResourceToken`、`TouchedContent`、`RequestedRuntimeFields`、`MutationHints`、分类函数与测试；`W/policy.rs:46,62` 的 `CommandClass`/`CoreRunIntent` 移到 intent.rs；`W/mutation.rs:108-129` 的 `MutationDomain`/impl 移到 source impact。`policy_for` 等工作流决策仍留 W。
- [x] `W/mutation.rs:35,321,331,350,370` 的 `ConfigDomain`、`MutationOutcomeKind`、`MutationConclusion`、`CheckRecord`、`MutationReceipt` 进 value.rs；`C/client/runtime.rs:130-155` 的 `RuntimeApplyReceipt` 进 `runtime_apply`.rs，仅放宽为 core 内实际消费者可用的 `pub(crate)`。`RuntimeRevision`/`RuntimeSnapshotStore`、`CoreSpec`/`LocalIpcSettings`、`AppliedConfigBinding`、`CandidatePortBindings` 仍由现有 owner 提供，不搬 manager/runtime 实现，不导出成协议 DTO。两 receipt 留 core，不进 D2e 值包。
- [x] `DomainChange`、`MutationRequest`、admission、Try/Confirm/Cancel、journal 与 `ApplicationMutationParticipant` 留 W。内部 receipt 消费者同迁：`W/{mod,mutation,participant,tcc,attempt}.rs`、`W/tests/{mod,mutations,recovery}.rs`、`C/state/{config_error,mutation/mod}.rs`、profiles actor（`:539`）、`C/client/core_lifecycle/workflow.rs:178,628`、`client/ports.rs:21,216,285`、`client/runtime_recovery.rs:33,151,243`、runtime store 与 tests。迁完 intent/impact 的源 owner、facade、workflow、core-lifecycle 与测试导入，不留 W 转发 re-export。
- [x] **不移动 notifications**：`CommitNotifications`/tuple/no-op（`C/client/effects/ports.rs:148-226`）、`ApplicationEffectFields`/`ClashEffectFields`、`EffectKind`（`effects/plan.rs:37,60,200`）留 client/effects。`MutationHints`.`requested_owners` 仍消费它（原 `W/impact.rs:214`）。D2f 的源规则仅禁止 state 直接引用 workflow/具体 client，允许 effects、jobs、runtime 值与验证契约；本卡尚有 coordinator → workflow 边，D2b 消除后才启用规则。
- [x] 无新 crate/dependency，core/config 边继续 `default-features = false`；`S/backend-boundaries.ts`/测试、`STATIC_GATE_PREFIXES`/覆盖和 musl 两清单不新增。记录路线图源契约归属，不声称 module gate 已实施。impact 的 macOS 测试条件原样，现有 Display/source/serde 不变。

**验证**

- `cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；§0.8 比较 impact/policy/runtime、`application_workflow`、source/client/profile integration 清单，合计不变，只登记搬移模块前缀。重点保留 `cancelled_profile_waiter_keeps_admission_and_reconcile_does_not_replay_interruption`（`W/tests/connection_policy.rs:715`）、`a_failed_save_restores_the_verified_runtime_baseline`（`W/tests/mutations.rs:1272`）、`a_restore_that_cannot_be_observed_is_not_a_clean_cancel`（`:2542`）、`the_shutdown_during_await_decision_waits_for_the_decision`（`W/tests/closing.rs:113`）。
- `rg 'application_workflow::impact|application_workflow::policy::(CommandClass|CoreRunIntent)|runtime::RuntimeApplyReceipt' backend/nyanpasu-core/src backend/tauri/src --glob '*.rs'` 退出码 1，另审阅相对/分组导入；`rg -n 'struct (MutationReceipt|RuntimeApplyReceipt)' backend/nyanpasu-core/src/state/mutation --glob '*.rs'` 正向各一处。coordinator 仍有具体 workflow client 不算本卡失败。
- 反向撤销一个新定义而保留迁后消费者，正常 `cargo check -p nyanpasu-core --lib` 必须失败；恢复旧路径定义使旧路径/唯一归属核对失败。bindings 三份零 diff，`ConfigDomain` 公开身份/serde 拼写不变；Linux 检查与 macOS CI 覆盖 impact 分支。

**待执行验证**

- [ ] impact 的 macOS 条件测试执行。（未完成：本地为 Windows，交 macOS CI）

---

## D2b — 按域注入 mutation participant factory

本卡是保行为接缝；不改 admission、vote、caller-drop、持久化或结算时序（已知 `RecoveryRequired` 例外留 D2c）。

- [x] 在 `C/state/mutation/ports.rs` 定义消费者自有 `MutationParticipantFactory<T>: Send + Sync + 'static`，`T: Clone + Send + Sync + 'static`，`make(&self, intent: MutationIntent, decision: DecisionHandle, settle: oneshot::Sender<MutationReceipt>) -> StateParticipant<T>`；intent 含原 `operation_id`、hints、class、impact（原 `C/state/mutation.rs:184-199`）。不放 engine/contracts，不加查找 actor 的接口。`W/participant.rs` 实现工厂，`DomainChange: From<StateChange<T>>` 只在此实现上，不反向泄漏给源 coordinator。
- [x] coordinator 改为 `MutationCoordinator<T>`，Ready 存 typed factory 与现有 effects 端口，不存 `ApplicationWorkflowClient`（原 `mutation.rs:31-38,54,61-67`）。保留 Pending、connect 只调用一次、test-support Isolated；按 application config、`ClashConfig`、Profiles 三域构造，不把 `session_state` 加入运行时参与。
- [x] 同迁四路径：`C/state/application.rs:164-177`、`clash_config.rs:121-134`、`profiles/actor.rs:481-494,1287-1300`；source 的 prepare/vote → local write/YAML → decision → settle 顺序不变。`C/client/mod.rs:322,327,523` 与 `new_typed_config_clients` 改为三份 pending/connection，由 facade 启动 workflow 后分别连接实现；`client/{application,clash_config,profiles}.rs` 的 args、isolated constructors、`test_support`、profile integration 和 W tests 同改。另显式迁 `C/client/startup_recovery_tests.rs:53-54` 的 `new_typed_config_clients`：三个域均注入各自的 isolated coordinator，不连接真实 workflow；启动恢复测试纳入本卡 §0.8 清单比较。
- [x] 保留 `W/participant.rs:103-117`：未送达是 Rejected，丢失 verdict 是 Failed/`OwnerUnresponsive`；不加 timeout，不因 settlement receiver 消失停止 owner。`W/mod.rs:267-272` 的 actor 等真实结果后记录/发送原样。新增测试（新）`participant_factory_receives_the_source_decision_and_intent`、`undelivered_mutation_is_rejected_but_lost_verdict_is_failed`、`dropping_the_settlement_receiver_does_not_cancel_owner_work`，用 typed clients 与 Notify/barrier，不能只测工厂 mock 或用 sleeps。
- [x] 无新 crate/dependency，已有 core/config defaults-off 与 test-support 接线不变；gate 集合、`STATIC_GATE_PREFIXES`、musl package/expectedTargets 不新增，源策略在 D2f 落地。同步路线图工厂接缝状态。

**验证**

- `cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；§0.8 清单 = D2a + 三新测试。保留 D2a 四例，以及 `a_lost_commit_notification_is_resolved_by_the_authoritative_decision`、`a_lost_rollback_notification_is_resolved_by_the_authoritative_decision`、`a_slow_source_write_is_waited_out_and_its_decision_settles_the_attempt`（`W/tests/mutations.rs:996,1041,1183`）、`a_conflicting_version_never_waits_for_a_settlement`、`every_settlement_arrives_whatever_the_journal_keeps`（`:4231,4376`）。
- `rg 'ApplicationWorkflowClient|DomainChange' backend/nyanpasu-state-engine/src backend/nyanpasu-core/src/state --glob '*.rs'` 退出码 1；`rg -n 'MutationParticipantFactory' backend/nyanpasu-core/src/state/mutation backend/nyanpasu-core/src/client/application_workflow/participant.rs --glob '*.rs'` 正向核对 consumer/implementation，不全仓禁止具体 workflow client。
- 反向把丢 verdict 改为 Rejected，使新增 verdict 测试失败；让 receiver drop 取消 actor 工作，使新增 caller-drop 测试失败；接错任一域 intent/decision，使工厂测试失败。bindings 三份零 diff；原错误对象和结构不变。

---

## D2c — `RecoveryRequired` 等待源权威决定

本卡是行为变更：遇到 `RecoveryRequired`，调用方的终态 receipt 与 journal 完成记录只在源事务权威决定之后出现；结论仍为 `RecoveryRequired`，不伪装成拒绝或干净回滚。

- [x] **先写失败测试再修实现**：在 `W/tests/mutations.rs` 新增 `recovery_required_settlement_waits_for_the_source_decision`（新）。新 fixture 在启动源操作前创建 settlement channel，并把 receiver 留在测试任务；不得直接复用直到事务结束才返回 receiver 的 `mutate_settling`（`:516,541-542`）。用真实 workflow/typed source 驱动无法确认恢复的 `RecoveryRequired`，不是伪造 receipt；包装 participant，在收到 prepare verdict 后门控向源返回 ACK。采用 `#[tokio::test(flavor = "current_thread")]`，保证发送 verdict 的 workflow poll 已运行到旧提前返回或新 decision wait 后，测试才被唤醒；若改用多线程，必须加显式观测握手证明该分支已执行，不能仅凭收到 verdict 或 sleeps。源仍被 gate 阻塞时断言 decision 未发布、receiver 的 `try_recv` 为 Empty、journal 无该 operation 的终态；旧分支须在 receiver/journal 断言上失败。释放 ACK 后断言权威决定已发布、receipt 为真实 `RecoveryRequired` 且只结算一次、journal 同 operation 终态一致；保存修复前失败和修复后通过输出。
- [x] 改 `W/tcc.rs:193-201` 的 `RecoveryRequired` 路径：保留 `:180` 先发 vote，再等待同一 `DecisionHandle`；源决定后才 conclude/return，不运行假定 baseline 已知的 Confirm/Cancel，不抹掉 error/cause。`W/mod.rs:269-272` 继续等待结果、记 journal、发送 receipt。不要在发 vote 前等 decision，也不要让 source 等 receipt 才发 decision，二者都会环等。
- [x] 新增 `dropped_source_publishes_abort_and_releases_recovery_required_settlement`、`shutdown_waits_for_recovery_required_source_settlement`（新），验证显式丢弃源事务 future 时 engine drop guard 发布 abort 后释放等待、shutdown 等源决定；外部 caller 离开仍不取消 owner 已开始的工作，不提前停止 actor。保留 resource evidence，不把 lost verdict 改拒绝。用户可见变化只限终态时点，不新增错误变体/自动重试。
- [x] 无移动/新包/依赖，Cargo、gate 规则/测试、`STATIC_GATE_PREFIXES` 与 musl 两清单不变；同步 design §5.3 的例外已修复状态及路线图，不改历史 V-24 对 fa7e5050b 的记录。无平台分支修改。

**验证**

- `cargo test -p nyanpasu-core recovery_required_settlement_waits_for_the_source_decision` 修复前须失败、修复后通过；再 `cargo test -p nyanpasu-core application_workflow` 与全 core 测试。§0.8 清单 = D2b + 三新测试；既有 `the_shutdown_during_await_decision_waits_for_the_decision`（`W/tests/closing.rs:113`）、`a_slow_source_write_is_waited_out_and_its_decision_settles_the_attempt`（`W/tests/mutations.rs:1183`）、D2b caller-drop/lost-verdict 测试通过。
- `rg -n 'RecoveryRequired|await_decision|request.answer|conclude_attempt' backend/nyanpasu-core/src/client/application_workflow/tcc.rs` 枚举并审阅两通道顺序，这是正向检查，不以退出码 1 判成功；本卡无旧路径删除，D2b scoped 无匹配检查继续通过。
- 反向恢复 `:193-198` 提前返回，首个新测试必须失败；把 wait 移到 vote 前，门控测试必须暴露未送达 vote（有界测试驱动/显式断言，不给生产加 deadline）。断言 Display/source/serde 与 D2b 相同，bindings 三份零 diff。

---

## D2d — 公开错误闭包的消费端接缝

本卡是保行为接缝；只消除后续值包的 engine/core-manager 类型依赖，不改变分类、Display/source 链或 wire。

- [x] 在 core 内分离 `C/state/mutation.rs:235-385` 的值定义与分类操作（D2a 后按符号定位）。`CommitAborted` 五个 skipped `ReplaceIfVersionError` source 字段（`:241,248,256,267,274`）改为 contracts 已有 `BoxedCause`；先按真实 engine 变体分类，再整体装箱，保留可 downcast 的原错误对象。`CommitAborted`::classify、`RuntimeAftermath`::of 改 source 模块自由函数，参数仍可收 engine error/内部 `MutationReceipt`，避免跨 crate 后给外部类型补 inherent impl。
- [x] `C/client/runtime_error.rs:37` 的 `CoreError` → `CoreFailure` 转换改 core-local 函数；所有 `source(from(CoreError, ...))` selectors 的消费者先 `map_err` 转 `CoreFailure` 再 context。`CoreFailure.kind` 直接引用中立 `nyanpasu-core-metadata::CoreErrorKind`（定义在 `backend/nyanpasu-runtime/crates/nyanpasu-core-metadata/src/error_kind.rs:13`；manager `src/error.rs:2` 只是 re-export），不复制枚举、不改 submodule。不能在 core 中保留两个外部类型之间的 From impl。`AckPayload`/`ack_of`/`refusal_of`（`runtime_error` `:256,267,274`）留 core acknowledgment adapter。
- [x] 同改非 selector 转换：`W/tcc.rs:1683,1684` 的 `submission_failure` 两个 `.into()` 显式调用 core-local 转换函数，分别覆盖 `NotSubmitted`/`Unchanged`/`Err` 与 `Unknown`；上述转换测试逐分支断言 kind/retryable/operation_id 不变。`W/ports.rs:59` 的 `RuntimeCheckUnavailable::Backend.kind` 同改为 metadata 自有 `CoreErrorKind`。`C/client/runtime_error.rs:26` 的 `CoreError` rustdoc 链接改为纯文本说明，不为修链接恢复 manager 导入。
- [x] `C/client/runtime.rs:818-827` 的 `From<&ApiError> for InterruptFailure` 移到 `C/actor_v2/api.rs`，与本地 `ApiError`（`:17`）同 owner；实现本体不变，`C/proxies.rs:260`、`W/tcc.rs:1170` 调用不变。D2e 后此 impl 仍可留 core，因为参数类型 `ApiError` 属于 core；只把值类型导入改为 foundation，不能让 control 为转换调用 client。分类回归覆盖四个 API 错误分支。
- [x] 准备 definition-only 模块供 D2e 原样移动：runtime 结果/降级、`RuntimeError`/`RestoreFailure`/`CoreFailure`、`PublishRuntimeError`、`PortField`/`PortResolveError`、`CoreSpecError`、`ExecutionHost`、`EvidenceGap`、`RuntimeCheckUnavailable`、`ProfilesError`/`ProfileContentError`/`MaterializationOperation`、`CommitAborted`/`RuntimeAftermath`/`NotReady`/`ConfigDomain`。在 core 内放 `runtime_values/`，旧路径不留 re-export，调用方同提交改导入。`ProfileFileError`/`SubscriptionFetchError` 仍在 contracts，`RuntimeBuildError` 留 transform；不移动运行时 IO、port resolver、`core_spec`、source actors 或内部 receipts。
- [x] 调用方同迁：`C/client/{runtime,runtime_error,runtime_inspection,ports,error,mod,profiles}.rs`、`core_lifecycle`、W 的 attempt/mutation/preparation/startup/tcc/workflow/tests、`C/state/{application,clash_config,config_error,mutation/mod}.rs`、profiles actor/error/jobs、`C/proxies.rs`、`service/profile_file/yaml.rs`、profile integration/`test_support`、`T/ipc.rs` 及 error assertions。测试对 `WriteConfig` cause 的 engine match 改为 downcast，不能删断言。
- [x] 补齐同提交消费者（D2e 继承全部清单）：control/fixtures 的 `C/actor_v2/mod.rs:53`、`facade.rs:16`、`api.rs:457`、`service_actor.rs:1246`、`test_support.rs:19`、`tests.rs:26`；binary resolution 的 `local_host.rs:101,118`；domain clients 的 `C/client/application.rs:16`、`clash_config.rs:17`、`clash_api.rs:63`、`hotkey/mod.rs:17`；`client/runtime_build.rs:13,21`、`runtime_recovery.rs:36`、`jobs.rs:198,206`；测试分支 `client/app_lifecycle.rs:104`、`geo_mode.rs:71`；`T/setup_tests.rs:16-18` 的 `endpoint::ExecutionHost`；workflow 的 `W/adapters.rs:79-80`（`RuntimeCheckUnavailable`、`ExecutionHost`）、`W/profiles.rs:3,10,14-21`（限定的 `runtime::Degradation*`）、`W/mod.rs:36` 与 `W/participant.rs:27`（`RuntimeError` 及 `OwnerUnavailableSnafu`/`OwnerUnresponsiveSnafu`）。混合导入要拆开：移走的错误类型与 selector 改从新定义 owner 导入，`ack_of` 留在 core 的 ack 适配处。同时迁限定 selector 引用与类型导入，逐文件搜旧路径（含 `runtime::Degradation*` 限定路径）；禁止以 re-export 补遗漏。上述生产/测试模块均纳入 §0.8 比较。
- [x] 新增（新）`boxed_commit_cause_preserves_display_and_source_chain`、`runtime_core_failure_conversion_preserves_kind_retryability_and_operation_id`、`runtime_error_serialization_preserves_existing_tags_and_fields`、`profile_commit_error_serialization_omits_operational_sources`、`interrupt_failure_mapping_preserves_all_api_error_categories`。覆盖 `RuntimeError` → `CoreFailure`、`ProfilesError` → `CommitAborted` → engine → 原 IO/YAML source 链，分类失败步骤与 serde skip 不变；Snafu selector 可见性只按真实消费者开放。
- [x] 无新 package；core 按需声明 metadata 直接依赖，删除 manager 依赖不是本卡目标；core/config 边仍 defaults off。gate/`STATIC_GATE_PREFIXES`/musl 两表不增加成员，运行既有策略测试；更新 design/路线图错误接缝状态。新模块采用 snafu，既有框架原样。

**验证**

- `cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；§0.8 清单 = D2c + 五新测试，只登记 definition/test 模块前缀变化；D2a–D2c 全部回归继续通过。`T/specta_export.rs:729-797` 的既有 shape assertions 保留；rpc-bindings.ts 仅一行 CoreError JSDoc 链接改纯文本，另两份零 diff；wire 不变（design V-66）。
- `rg 'nyanpasu_core_manager|ReplaceIfVersionError|StateChangedError|crate::client|application_workflow' backend/nyanpasu-core/src/runtime_values --glob '*.rs'` 退出码 1，另审查相对/分组导入；`rg -n 'classify|downcast_ref' backend/nyanpasu-core/src/state/mutation --glob '*.rs'` 正向确认分类仍在消费边界。旧定义路径按调用清单逐项搜，不全局禁止仍合法的 `runtime_error` adapter。
- 反向把 source 装箱换字符串，source-chain/downcast 测试失败；丢 `operation_id`/kind/retryable，转换测试失败；改 serde tag 或 skipped source 字段，shape 测试失败。正常 `cargo check -p nyanpasu-core -p clash-nyanpasu --lib` 不开 test-support 也须编译，不用全 feature 掩盖生产 selector 缺失。

---

## D2e — 公开运行时值与错误闭包下移

本卡是纯移动；D2d 已切断 operational source/conversion 的反向边，wire、Display 与 source 链不变。

- [x] 新建 `nyanpasu-runtime-values`，把 D2d 的 `C/runtime_values/` definition-only 模块与对应值/错误测试原样迁到 V，删除 core 旧模块，不留转发。清单对应基线：`C/client/runtime.rs:640,649,659,758,770,810,832` 的 `CommitReceipt`/`RuntimeCommitStatus`/`MutationOutcome`/Degradation/`DegradationReason`/`InterruptFailure`/`DegradationPhase`；`client/runtime_error.rs:29,63,230` 的 `CoreFailure`/`RuntimeError`/`RestoreFailure`；`runtime.rs:343` `PublishRuntimeError`；`client/ports.rs:27,37` `PortField`/`PortResolveError`；`actor_v2/local_host.rs:88` `CoreSpecError`；`actor_v2/endpoint.rs:34` `ExecutionHost`；`W/mutation.rs:35,205` `ConfigDomain`/`EvidenceGap`；`W/ports.rs:47` `RuntimeCheckUnavailable`；`state/profiles/error.rs:17,37,52` `ProfileContentError`/`MaterializationOperation`/`ProfilesError`；`state/mutation.rs:227,235,291` `NotReady`/`CommitAborted`/`RuntimeAftermath`，及纯格式化辅助。
- [x] 完整闭包允许下行引用 config 的 `ProfileId`/paths/validation/`PickPortError`、contracts 的 profile/install/service/version 错误、transform 的 `RuntimeBuildError` 及其 config pipeline/log/failure 值、中立 `CoreErrorKind`；原被 skip 的 IO/YAML/JSON/url/`JoinError` 等源照旧，不为「纯值」删除诊断。`MutationReceipt`/`RuntimeApplyReceipt` 留 `C/state/mutation/`，内部 `OperationId`/`CoreSpec`/`LocalIpcSettings`/binding 不进入值包。`InterruptFailure` 的 `ApiError` 转换、`CoreError` 转换、commit 分类与 ack 适配仍由 core 实现/调用；不在值包引入 core 类型。
- [x] D2d 全部消费者同提交直接 import 新包，包括 `C/proxies.rs:11-12`、source snapshots/outcomes、profile jobs、client/facade/workflow/lifecycle、tests、T/ipc.rs。`ProfilesError`::report 的纯方法随定义移动，只为跨包生产消费者开放所需可见性，不为测试暴露整个实现模块。`ConfigError`、`HotkeyParseError` 与其他 E1b DTO 不属于本卡新增闭包，留原 owner；E1b 另设计接收端 DTO，不给 Serialize-only operational error 强塞 Deserialize/伪造 source。
- [x] 按 §0.5 逐项核定跨包可见性：`RuntimeError::core_kind`（`C/client/runtime_error.rs:331`，消费 `W/tcc.rs:998`）、`retryable`（`:339`，消费 `C/client/core_lifecycle/workflow.rs:425`）及 `ProfilesError::report` 按实际消费者开放；`core_failure` 仅由迁后方法使用时仍私有。审阅 `RestoreFailure`（runtime_error `:229`）、`PublishRuntimeError`（runtime `:342`）、`ProfileContentError`（profiles/error `:15`）的受限 Snafu selectors，以及默认可见性的 `CoreSpecError`/`PortResolveError` selectors；只公开生产调用所需 selector，或同步改等价本地构造。不公开整个模块来迁就测试。正常无 test-support 的 core/tauri 检查与限定 selector 旧路径核对必须通过。
- [x] workspace/消费者 manifest 加包；core/tauri 普通依赖新值包、defaults off。值包实际 workspace 下行边为 config（defaults off，按实需显式 feature）、contracts、profile-transform（defaults off），另直接依赖独立 runtime 的 core-metadata；保留原 serde/rc、specta、snafu、url、IO/YAML/JSON/atomicwrites/Tokio 错误等所需库版本，不依赖 core/engine/core-manager。纯测试在值包 unit tests，core conversion/classification 测试原地；值包不加 core dev-dependency。
- [x] `S/backend-boundaries.ts` 将值包加入 `FOUNDATION` 上层；禁止所有 `CORE_FAMILY`（含 engine）、`IMPLEMENTATION`、GUI、core-manager。原 config/contracts/persistence/transform 四叶均显式禁止值包，不能只更新 config 的自动集合规则。`backend-boundaries_test.ts` 正例 core → values、values → config/contracts/transform/metadata；负例逐叶 → values、values → 各 core 族/engine/实现/GUI/core-manager，含直接/传递路径。现有 contracts → transform 禁令不放宽，不用尚不存在的 protocol 当真实 cargo tree 根。
- [x] `S/policy.ts` 加 `backend/nyanpasu-runtime-values/src/`，覆盖矩阵同加；musl 同加 `-p nyanpasu-runtime-values`、`lib:nyanpasu_runtime_values`。同步 backend-packages/architecture 的上层 foundation/反向禁令、路线图 D2 值闭包进度；不说 E1b 已完成。

**验证**

- `cargo test -p nyanpasu-runtime-values`、`cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；§0.8 三侧合计清单 = D2d，只允许 definition/test 所属包/模块前缀变化；五个错误闭包测试、D2a 四例与 D2c 三例保留。三份 bindings 零 diff，尤其 `MutationOutcome`<()>、递归 profile/runtime cause、蛇形 tag 与字段不变。
- `rg 'nyanpasu_core::|nyanpasu_core_manager|nyanpasu_state_engine|ApplicationWorkflowClient|RuntimeApplyReceipt|struct MutationReceipt' backend/nyanpasu-runtime-values/src --glob '*.rs'` 退出码 1；`rg 'crate::runtime_values|nyanpasu_core::runtime_values' backend/nyanpasu-core/src backend/tauri/src --glob '*.rs'` 退出码 1；分组旧导入另审阅。core acknowledgment adapter 和 source 分类正向保留，不删业务断言清搜索。
- 反向加 values → engine/core-manager 或任一 foundation 叶 → values，gate 失败；把 engine error 具体类型塞回值包，依赖/编译检查失败；删新静态 prefix，覆盖测试失败。`pnpm run test:scripts`、两门禁、`pnpm run test:backend-musl` 同时验证新增目标注册。
- 只迁定义，不移动基线 Unix runtime IO/macOS `local_host` 装配；正常生产 consumers 检查不启用 test-support；Linux 检查及 macOS CI 继续验证平台条件下类型可用。

**待执行验证**

- [ ] `pnpm run test:backend-musl` 发现并运行 runtime-values 新 lib 目标。（未完成：本地 Docker daemon 不可用，待 Docker 环境执行）
- [ ] Linux/macOS 平台消费者与条件分支执行验证。（未完成：本地为 Windows，交 Linux/macOS CI）

---

## D2f — core-control 与 application 的模块策略

本卡是保行为接缝（静态边界验证），不移动 owner、不建 control/workflow crate、不改变运行结果。

- [x] 明确集合：control = `C/actor_v2/`、`C/clash/`、`C/proxies.rs`、`C/connections.rs`；application = `C/client/`，含 facade、`application_workflow`、`core_lifecycle`、effects；源 owner = `C/state/`。application 可调用 control，control 禁止引用 client，state 禁止直接引用 client::`application_workflow` 或其具体 `ApplicationWorkflowClient`；source → effects/jobs/runtime 值/验证契约仍允许。`C/actor_v2/service_actor.rs:34` 的 service compat 边允许，不把所有根模块强行分入 control/application。
- [x] `CommitNotifications`/`EffectKind` 等留 effects（D2a 已明确）；D2e 已把 `C/proxies.rs:11-12` 的结果/降级导入改为 foundation，此卡核对，不重复移动。factory 位于 state/mutation/ports.rs，workflow 实现消费它；内部 receipts 留源参与模块。不能声称消灭全部 state → client 边或证明模块传递无环。
- [x] 新建 `S/core-module-boundaries.ts` 与 `S/core-module-boundaries_test.ts`，接入 `S/backend-boundaries.ts:155-192`，真实违规使现有 gate 非零。`S/lexer.ts:1,48` 仅负责剥离注释/字面量，不提供解析或名称解析。实现有限语法守卫，不做类型/trait/宏展开或调用图解析：支持普通标识符路径、`crate`/`self`/重复 `super`、嵌套 grouped use-tree（含 `self`）、`as` 别名、`pub`/`pub(crate) use` 与限定路径引用；按文件模块和 inline `mod` 的词法作用域归一化，块内 use 只在该块生效，cfg 分支均检查。
- [x] provider 范围为 `C/` 全部普通 `.rs` 模块（由 `lib.rs` 的普通 `mod` 声明及 inline 模块建立索引），只索引模块声明和 import/re-export，不追踪函数实现依赖。消费规则仍仅作用于上列 control/state 路径。对消费路径触达的同 crate provider 导出沿显式别名/re-export 展开，最多 8 跳；循环、歧义、超限、不能定位的同 crate provider 或不支持的路径形式明确报错，不默默放行。普通外部 crate 根（如 `nyanpasu_ipc::client`）不当成 `crate::client`，仍交 Cargo gate 管。允许 `service` 的本地定义桥接，但若它 re-export 禁止的 client 类型，须拒绝。
- [x] policed consumers 及其触达的 provider 导出拒绝不支持的 glob；例外为目标模块也在受检范围内的 `super::*`/`self::*`，继承并解析其 alias bindings，不能绕过父作用域别名检查（design V-64）。provider re-export glob 仍拒绝，受检路径任意位置的 `r#` 亦拒绝；宏生成 import、动态/不支持的模块定位（如未登记的 `#[path]`）、隐藏来源的 `type Alias`/`extern crate ... as ...` 给出路径/行号诊断。保留允许的 super/self glob，不再要求全部改显式导入；不扩展成通用 Rust resolver，新增支持形式前先补正反 fixture。
- [x] 新增策略测试（新）`application_may_import_control`、`control_cannot_import_client`、`source_owners_cannot_import_workflow`、`source_owners_may_consume_effects_and_value_contracts`、`module_policy_resolves_grouped_relative_and_aliased_paths`、`module_policy_checks_tests_and_ignores_comments_and_strings`、`module_policy_allows_service_bridge_and_external_ipc_client`、`module_policy_rejects_indirect_forbidden_reexports`、`module_policy_rejects_globs_cycles_and_unsupported_forms`。覆盖允许 service 本地定义桥/外部 IPC client，禁止经 root/provider/多级 alias 引入 workflow，inline 与块作用域，8 跳边界、歧义和失败诊断；真实仓库扫描必须通过。Cargo gate 保留。
- [x] 无新包/Cargo feature，core/config defaults-off 不变；`STATIC_GATE_PREFIXES`/覆盖和 musl 两清单不新增。同步 architecture/backend-packages 精确模块规则与路线图 D2 完成条件；独立 control crate 等真实消费者出现后另立闭包盘点卡，server 需要两者、CLI 两者都不链接，不声称编译收益。

**验证**

- `pnpm run test:scripts`、`pnpm run lint:backend-boundaries`、`pnpm run lint:architecture-ledger`；Rust §0.8 清单无增删，`cargo test -p nyanpasu-core` 保留 `a_new_router_generation_is_not_evidence_that_a_submission_finished`（`C/actor_v2/facade.rs:1428`）、D2a cancellation/restore/closing 四例与 D2c 三例。新增的是上述九个脚本策略测试及 super/self glob、继承别名、raw identifier 三个回归，共 12 例。
- `rg 'ApplicationWorkflowClient' backend/nyanpasu-state-engine/src backend/nyanpasu-core/src/state --glob '*.rs'` 退出码 1；`rg 'crate::client|client::runtime' backend/nyanpasu-core/src/actor_v2 backend/nyanpasu-core/src/clash backend/nyanpasu-core/src/proxies.rs backend/nyanpasu-core/src/connections.rs --glob '*.rs'` 退出码 1；这两条只是 PR-time 辅助，分组/别名由新策略测试验证。
- 反向在 control fixture 引入 `use crate::{client as app};` 后引用 app，或 source fixture 经 re-export 引用 workflow，策略测试须失败；真实源增加同样导入须让 gate 非零；允许的 effects import 不能误报。bindings 三份零 diff；无 error/platform 语义变动。

---

## A2b-a — 显式脚本扩展接缝

本卡是保行为接缝；默认 JS/Lua、HTTP/文件/console、缓存、日志、错误结果与线程语义不变，不在本卡修缓存策略或加入不支持结果。

- [x] 在 `J/extensions.rs` 定义 runner 消费的扩展工厂，构造参数显式含 `JavaScript` loader/console、Lua 标准库/console 配置；`ScriptDirs`（`J/js.rs:32-47`）移到 engine-independent `J/runner.rs`，lib 直接导出新定义，不保留旧 js 路径。迁 `J/adapter.rs:29,67`、`runner.rs:53-77`、`js.rs:104-157,209-217`、`lua/mod.rs:10-21` 与 `T/setup.rs:422-429` 构造链。保留 core 的 `W/ports.rs:9` `ScriptRunnerFactory`、`C/client/runtime_build.rs:93-102` blocking-thread 构造，不让 scripts 依赖 core。
- [x] 在 `B/module/http.rs` 定义消费者自有 `HttpModuleSource` 端口，收 URL、异步返回现有 `CachedItem`（`:30-35`）或保留 source 的模块自有 snafu `LoadModule` 错误；Boa MIME/Module parse（`:58-82`）留 loader。把缓存路径/时钟、读取、fetch、写回机制（`:43-55,95-195`）与 reqwest client 构造移到 `T/adapters/script_extensions/http.rs`，shell 显式构造含 client、cache 路径、`max_age`、clock/执行 handle 的实现，再注入 loader factory。新错误保留原失败文本/source，不借接缝改变脚本错误分类；窄聚合端口避免把 HTTP cache 拆成多套服务或增加 crate。
- [x] `T/adapters/script_extensions/mod.rs` 装配本地/HTTP/builtin loader 和 console/Lua 扩展；工厂跨线程可 Send/Sync，实际 Boa/Lua/Rc loader 在使用/销毁它的线程创建。`J/js.rs:177-178` 的 root module identity/insert 保留；`J/js.rs:273-289` 的日志收集与 blocking 执行原样。显式注入 Tokio handle 代替 `B/module/http.rs:151` 的隐藏 utility runtime，不能移动 Rc context 到另一个线程或因 waiter drop 丢失已开始的工作。
- [x] 保留 redirect limit 5（http `:154`）、MIME fallback、缓存命中也写回（`:180-195`）、旧 cache key/TTL 和目录/相对 import 行为；Lua 默认仍 `ALL_SAFE`（`J/lua/mod.rs:12`），不趁机收紧脚本能力。console 仍捕获异常前日志并隔离并发运行。普通 script error 字符串化暂留 adapter，A2b-b 只给不可用引擎保留 typed 分支。
- [x] 迁现有 HTTP loader 测试（`B/module/http.rs:203` 的 `test_http_module_loader`）与机制 fixture 到 shell adapter，改受控 source/server；Boa 侧保留解析/注入测试。scripts 的 adapter/js/lua/console/`ordered_map`、boa builtin tests 和 transform `runtime_builder/script_tests.rs:17`、`runtime_builder/golden.rs:65` 的真实引擎消费者同改构造，不留旧默认构造兼容入口。
- [x] 中立测试扩展由各消费包的小型本地 fixture 显式装配：scripts 的 `cfg(test)` helper 与 transform 新 `runtime_builder/test_extensions.rs`（仅 `cfg(test)`，供 script_tests/golden 共用）使用真实引擎、临时目录、显式本地/builtin loader、受控 HTTP source/clock 与 console/Lua 配置；调用公开扩展构造接口，不依赖 Tauri，不新增共享 test-support feature。transform 的 scripts dev-dependency（`backend/nyanpasu-profile-transform/Cargo.toml:27`）只按需启用引擎。禁止 Tauri dev-dependency、隐式默认装配或替代 default-constructor shim；golden 输入/期望 fixture hashes 原样，§0.8 连同 golden 全清单比较。HTTP 专项测试使用受控服务，不让 golden 意外访问公网。
- [x] 无新 crate；`boa_utils` 删除不再使用的 reqwest/缓存机制依赖，shell 按实际使用声明 reqwest、async-fs/postcard 等迁入依赖与 loader 构造所需 Boa 依赖，保留版本/TLS。core/config 边 defaults off；不增加 scripts → core/net/其他实现边。`boa_utils` 的新端口不依赖 shell；`S/backend-boundaries.ts` 为 `boa_utils` 补 GUI/core-family/实现依赖禁令及正例 helper → 中立依赖、负例 helper → core/tauri/net，避免经 helper 绕过实现层。
- [x] `S/policy.ts` 与覆盖矩阵加入 `backend/boa_utils/src/`，仅为 `B/module/builtin.rs:14` 的既有不可变 `BUILTIN_MODULES` 登记 static 名称/路径/理由，不豁免运行时服务；musl `-p boa_utils` 与 `lib:boa_utils` 同加，迁入 shell 的机制测试由 tauri CI 跑，不虚报在 musl。同步路线图 A2b 接缝状态，不改引擎 feature 状态。

**验证**

- `cargo test -p boa_utils`、`cargo test -p nyanpasu-scripts`、`cargo test -p nyanpasu-profile-transform`、`cargo test -p clash-nyanpasu`；§0.8 四侧 inventory 合计 = 原清单 + 新测试，HTTP 机制测试仅改归属。scripts adapter 既有五例 `runs_javascript_transform_and_captures_logs`、`failing_script_returns_error`、`eval_item_errors_name_the_expression`、`eval_item_expr_keeps_the_item_key_order`、`eval_item_predicate_and_expr_use_lua_item_global`（`J/adapter.rs:122,146,159,178,195`）保留；js 的 `scripts_run_from_the_injected_dir`、`logs_written_before_a_throw_survive_the_failure`、`concurrent_runs_keep_their_logs_apart`（`:414,458,610`）、`test_process_honey_with_fetch`、`test_process_honey_with_builtin_modules`（`:804,869`）保留。完整 js/lua/console/`ordered_map`/builtin 清单按 §0.8 导出，不用重点测试代替全清单。
- 新增（新）`module_fetch_uses_the_injected_client`、`module_cache_expiry_uses_the_injected_clock`、`cache_hit_preserves_existing_refresh_behavior`、`relative_imports_resolve_under_the_injected_root`、`root_module_identity_survives_loader_injection`、`module_load_error_preserves_display_and_source_chain`；网络/时钟均可控，不依赖公网或 sleeps。
- `rg 'reqwest::Client|nyanpasu_utils::runtime' backend/boa_utils/src --glob '*.rs'` 退出码 1；`rg 'nyanpasu_core::|nyanpasu_net::|tauri::' backend/nyanpasu-scripts/src backend/boa_utils/src --glob '*.rs'` 退出码 1；grouped imports 另审阅。反向自建 client、忽略注入 clock、丢 root insert，分别使新增 fetch/cache/identity 测试失败；删除 boa prefix 使静态覆盖失败。两门禁、脚本测试、musl runner；bindings 三份零 diff，无新 wire 类型；没有新增 Unix/macOS 分支。

**待执行验证**

- [ ] `pnpm run test:backend-musl` 发现并运行 boa_utils 新 lib 目标。（未完成：本地 Docker daemon 不可用，待 Docker 环境执行；shell 机制测试不计入 musl）

---

## A2b-b — 引擎 feature 与不可用错误贯通 wire

本卡是行为变更，**一个提交**同时提供引擎 feature、typed port error、所有 executor 分支传播、wire/bindings 与前端消息。默认双引擎、shell 显式选择；任何可选 engine-off 构建都必须拒绝需要该引擎的操作，不能存在先开放 feature、后修 false/no-op 回退的中间提交。用户看到含 runtime/operation 的结构化失败；普通已启用引擎的脚本异常保持原语义。

- [x] `backend/nyanpasu-scripts/Cargo.toml` 新增 `default = ["javascript", "lua"]`；javascript 控制 Boa/`boa_utils`/oxc 等依赖与模块，lua 控制 mlua 与 Lua 模块；`J/lib.rs:7-10`、runner 注册（`:70-77`）、adapter run/eval_item_* 与 engine-specific tests 同 gated。公共 `ScriptDirs`/`ConfigValue`/console log 容器不依赖某个引擎类型；无引擎仍能构造 runner 并返回 typed 不支持。
- [x] shell 对 scripts 依赖 `default-features = false`，通过 shell 的 `script-javascript`/`script-lua` feature 显式转发，shell 默认均启用；A2b-a 新增的 Boa loader/HTTP adapter 依赖和模块随 shell javascript feature gated，Lua adapter 随 lua gated。core 不增加 JS/Lua feature、依赖或 engine 探测；transform 的 scripts dev-dependency 按测试所需显式启用引擎，不让多包测试 feature unification 冒充 scripts 单包 feature-off 验收。
- [x] 在 `Q/runtime/executor/script_error.rs` 新增并由 executor 导出 `ScriptExecutionError`（snafu）与 `ScriptOperation::{Transform, ItemPredicate, ItemExpression}`；错误变体 `StartEngine { runtime: ScriptRuntime, operation: ScriptOperation }`，Display 明确 build 中引擎不可用。serde/Specta 固定形状：错误 kind `snake_case`，operation `snake_case`，runtime 沿用 `ScriptRuntime`；本提交同时接入 pipeline/wire，按完整闭包重新生成 bindings。
- [x] adapter 先检查 runtime 是否编入，再执行 config 转换/脚本；不可用时直接装箱此错误到 `PortError`，不经 `J/adapter.rs:72` 的 `to_string`。Lua-off 的两个 overlay 入口（`:76-104`）同样返回 typed `StartEngine`，不返回 false/原 item/Ok，不把普通启用引擎的语法/执行错误误判为不可用；原 `PortError` 别名与其他内容读取端口不改。
- [x] 新增（新）`javascript_disabled_returns_start_engine_error`、`lua_disabled_returns_start_engine_error`、`lua_disabled_predicate_returns_start_engine_error`、`lua_disabled_expression_returns_start_engine_error`、`enabled_engine_errors_are_not_misclassified_as_unavailable`、`start_engine_error_keeps_display_and_serialized_fields`，分别按 feature 分组。默认双引擎的既有行为不变。
- [x] `Q/runtime/executor/error.rs:19` 新增 `RuntimePipelineError::ExecuteScript { source: ScriptExecutionError }`（wire `execute_script`，source 不 skip）；config 的 transform/overlay 路径遇到 typed `StartEngine` 向上传播，其余 `PortError` 保持原日志/失败策略。修改 `executor/mod.rs:184,226-233,354-374` 及 `overlay.rs:164-174,211-232` 的完整返回/调用链，含 builtin、composition-owned transforms 及其 direct-file base/contributors 的 scoped transforms（`Q/runtime/executor/scoped.rs:94`、`compose.rs:99-101`）；保留 `Q/profile/profiles.rs:510` 对嵌套 composition 的拒绝；不在最外层从错误字符串识别引擎。Result 传播与调用方同提交，源码不留成功 fallback。
- [x] transform 的 `RuntimeBuildError`::`RunPipeline`（`backend/nyanpasu-profile-transform/src/runtime_builder.rs:44`）保留原 wrapper，D4 后 values 的 `RuntimeError`::`BuildRuntime` 保留 source；core 分类/IPC 映射保留结构，不转 unknown 文本。测试经真实 executor → `RuntimeBuilder` → 公共错误序列化路径；包含非 strict transforms 的默认构建，不能只在 strict 模式拒绝。
- [x] config 新增（新）`disabled_transform_engine_is_a_structured_pipeline_failure`，在 `Q/runtime/executor/tests/{compose,include_all,builtin}.rs` 的本地 helper 中参数化 global、scoped、composition-owned、direct-file base scoped、每个 direct-file contributor scoped、builtin 路径；逐项覆盖独立调用/吞错点（`executor/mod.rs:226-233,354-374`、`scoped.rs:94`、`compose.rs:101`）。非 strict 默认执行下，每个 case 断言 `StartEngine` 的准确 runtime 与 `operation = Transform`，不能只复用 global 分支证明 builtin。
- [x] `Q/runtime/executor/tests/overlay.rs` 新增（新）`disabled_lua_filter_does_not_remove_an_item`、`disabled_lua_when_does_not_become_a_false_predicate`、`disabled_lua_expression_does_not_become_a_successful_noop`：分别断言 `ItemPredicate`/`ItemPredicate`/`ItemExpression` 及 Lua runtime；expression fake 必须先对 `when` 回 `Ok(true)`，再对 `eval_item_expr` 回 `StartEngine { runtime: Lua, operation: ItemExpression }`，并用调用计数证明进入表达式分支。三例均在非 strict 模式拒绝整次构建，不能只断言日志。新增 `ordinary_script_failure_keeps_existing_transform_semantics`（新）固定普通启用引擎错误的原回退行为。
- [x] transform 的 `runtime_builder.rs` 新增 `unsupported_engine_error_reaches_runtime_build_wire_shape`（新），从实际 executor 失败取得 `RuntimeBuildError` 再断言下层 wrapper；不得为此增加 transform → values/core/tauri 依赖。`J/adapter.rs` 新增 `disabled_engine_real_runner_reaches_pipeline`（新），真实 runner 调 executor：无引擎/单引擎配置断言禁用引擎的 typed 失败，默认双引擎配置断言启用引擎正常执行；覆盖单包四组 feature 配置；不新增 integration target。
- [x] core 的 `W/tests/mutations.rs` 新增 `unsupported_engine_is_rejected_before_source_persistence`（新）：经实际 `RuntimeBuilder` 失败驱动 config/profile 两类源 mutation，typed script fake 只在端口返回精确 unavailable；断言实际返回错误内 `RuntimeError::BuildRuntime` → `RuntimeBuildError::RunPipeline` → `RuntimePipelineError::ExecuteScript` → `StartEngine` 的 runtime/operation、source 链以及持久化/源版本未变，不只检查不落盘。若从 `ConfigError`/`ProfilesError` 进入，逐层断言实际 commit/aftermath cause，不手工拼造上层错误。
- [x] `T/ipc.rs` 的现有 unit-test 模块新增 `unsupported_engine_mutation_serializes_typed_ipc_envelope`（新）：用 shell 侧本地 fixture 执行上述实际失败的 config/profile mutation，将其真实返回错误经现有映射转换为 `IpcError`（`:55-72`）并序列化；逐项断言 config/profiles domain、commit/runtime/build/pipeline/script 嵌套、runtime/operation 和 message/detail 保留。共享行为可用既有 typed test 接口，不增加下层 → Tauri dev-dependency，不用手工构造 RuntimeError 替代实际失败构建。
- [x] 修订 `Q/runtime/executor/ports.rs:27` 的确定性契约：相同输入且注入扩展的网络/文件/时钟等观测相同时回复一致；不承诺 HTTP/缓存/`ALL_SAFE` 的无条件重放，不做冻结能力模式。同步 design §5.0 与路线图 A2b 状态，不改脚本已有能力/TTL。
- [x] 重新导出三份 bindings；只在 `frontend/nyanpasu/src/utils/ipc-error.ts:639-665` 的 pipelineMessage 加 `execute_script` 分支及私有脚本错误 mapper。新增 `error_runtime_script_start_engine` 消息，参数 runtime/operation；`frontend/nyanpasu/messages/en.json`、`ja.json`、`ko.json`、`ru.json`、`zh-cn.json`、`zh-tw.json` 六份齐全。通过 `pnpm run web:build` 的 Paraglide 插件（`frontend/nyanpasu/vite.config.ts:74-76`）重编译，不手改 src/paraglide，不改页面/组件。
- [x] `frontend/nyanpasu/tests/ipc-error.browser.test.ts` 新增参数化 `script_engine_unavailable_uses_its_message`（新），经公共 ipcErrorMessage 验证 runtime 以及上述 config/profile mutation 的实际 envelope 形状，覆盖两个 runtime、三种 operation 的消息，不为测试导出私有 mapper。前端例外按 design §9 A2b-b，仅此 mapper/消息/回归与生成 bindings。
- [x] 无新 crate；新增/可选依赖与 config、core、shell defaults-off 按本卡 feature wiring 同步；gate/prefix/覆盖/musl package 与 expectedTargets 无新增，新增测试使用既有 unit targets。运行默认 musl 和 scripts feature 矩阵，不用全 feature 替代；文档记录 A2b 已交付及下列待执行验证。

**验证**

- 分别运行并按 §0.8 保存各配置非空清单，不把互斥测试跨配置比较：`cargo test -p nyanpasu-scripts --lib`；`cargo test -p nyanpasu-scripts --lib --no-default-features`；`cargo test -p nyanpasu-scripts --lib --no-default-features --features javascript`；`cargo test -p nyanpasu-scripts --lib --no-default-features --features lua`。再跑 config、transform、tauri 既有测试；默认 inventory = A2b-a + 本配置新测试，禁用引擎的测试逐项说明条件，不静默消失。
- `cargo tree --offline -p nyanpasu-scripts --no-default-features -e normal,build` 不含 Boa/mlua；加 `--features javascript` 不含 mlua，加 `--features lua` 不含 Boa/oxc。shell 分别 `cargo check -p clash-nyanpasu --lib --no-default-features`、追加 `--features script-javascript`、追加 `--features script-lua`，再检查默认配置，证明扩展模块与依赖转发可裁剪。`rg -n 'StartEngine|eval_item_predicate|eval_item_expr|to_string' backend/nyanpasu-scripts/src/adapter.rs` 逐支审查 typed 分支先于字符串化；旧路径检查沿用 A2b-a。
- 反向把 typed 不支持转 String、返回 false/原 item，相关新测试失败；cfg 外引用 Boa/mlua，无引擎编译失败；删 shell feature 转发，对应配置装配检查失败。新错误 Display/serde 字段测试通过；bindings/frontend 按下列 wire 预期同时变更；无新平台分支。
- `cargo test -p nyanpasu-config`、`cargo test -p nyanpasu-profile-transform`、`cargo test -p nyanpasu-runtime-values`、`cargo test -p nyanpasu-core`、`cargo test -p clash-nyanpasu`；运行本卡四组 scripts feature 命令，按 §0.8 各清单 = A2b-a + 本卡所属新增测试。保留 `build_failures_reach_the_wire_with_their_context`（`backend/nyanpasu-profile-transform/src/runtime_builder.rs:528`）、`builder_runs_a_managed_script_through_the_platform_adapters`（`backend/nyanpasu-profile-transform/src/runtime_builder/script_tests.rs:17`）、A2b-a 日志/key-order/overlay 五例。
- wire 预期 diff **仅**新增 `ScriptOperation` 的 transform/`item_predicate`/`item_expression`、`ScriptExecutionError` 的 `start_engine { runtime, operation }` 和 `RuntimePipelineError` 的 `execute_script { source }`；`ScriptRuntime` 既有拼写不变。RPC bindings 新增这些闭包；tauri/query 文件若仅引用 RPC 类型则应零 diff，不强造差异；命令名/参数、`MutationOutcome`/`RuntimeError` 既有字段/tag、普通失败 shape 不变。下层 wrapper、core typed cause 与 Tauri IPC envelope 三层测试分别断言 Display/source 链/JSON 嵌套；浏览器参数化 fixture 与 Tauri 序列化形状对应。
- `pnpm run web:build`、`pnpm --filter @nyanpasu/rpc typecheck`、`pnpm --filter @nyanpasu/query typecheck`、`pnpm --filter @nyanpasu/nyanpasu typecheck`、`pnpm run test:frontend`；核对六种 locale 都有同名/同参数消息，不用 typecheck 代替浏览器行为断言。
- `rg -n 'StartEngine|ExecuteScript|downcast_ref' backend/nyanpasu-config/src/runtime/executor backend/nyanpasu-scripts/src --glob '*.rs'` 正向审阅 typed 传播；`rg 'reqwest::Client' backend/boa_utils/src --glob '*.rs'` 仍退出码 1；`rg 'crate::runtime_values|nyanpasu_core::runtime_values' backend/nyanpasu-core/src backend/tauri/src --glob '*.rs'` 退出码 1，避免在 D4 旧路径补新变体。
- 逐个反向恢复 global/scoped/composition-owned/member-scoped/builtin 的吞错分支，相应参数化 case 必须失败；恢复 overlay false/no-op，三个独立测试失败，尤其 `when = Ok(true)` 的表达式 case；把上层错误变成 unknown/字符串，core/Tauri envelope 测试失败；只在 strict 模式拒绝，默认模式测试失败；删 mapper 分支/某语言消息，typecheck/消息回归失败。两门禁、脚本测试、musl runner；无新 Unix/macOS 分支。

**待执行验证**

- [ ] `pnpm run test:frontend` 执行 `script_engine_unavailable_uses_its_message` 浏览器回归。（未完成：本地缺少 Playwright 浏览器，未完成浏览器行为断言）
- [ ] `pnpm run test:backend-musl` 默认配置执行。（未完成：本地 Docker daemon 不可用，待 Docker 环境执行；本地四组 scripts 验收不替代 musl）

---

## 5. Phase E 的 PR 划分

本节细化 design §5.4；源码锚点记录于 `b70bb66d7`（A、B、C、D1 已完成），按符号定位，不能用行号覆盖后续 D 提交。偏差见 design §8.1 V-30–V-63，决策见 §8.2 O-18–O-46；O-13 的维护者选择与全局约束 §0 适用。每卡一个可独立编译、测试通过的提交，同 PR 按箭头顺序；纯移动、保行为接缝与行为变更分别提交。以下均未实施，新增文件/符号/测试是计划项。

- 固定实施顺序：PR-D2 → PR-D3 → PR-D4 → PR-A2b → PR-E1 → PR-E2 → PR-E3 → PR-E4 → PR-E5 → PR-E6 → PR-E7 → PR-E8；所有 E 卡均不得先于 D/A2b 合入。D/A2b 保留 §4 现有消费者与验证清单，E 一律消费已存在的 D2e `nyanpasu-runtime-values` 及 A2b 扩展工厂；E1b-a 必须在 E3e 后，才能验证 `local-server` 与 typed-client 消费侧。此为实施顺序，不新增下层到 RPC 的架构依赖。E5/OpenWrt 与 LAN ticket login 继续暂停，不新增实施卡。
- 缩写：`R/` = 计划的 `backend/nyanpasu-rpc/src/`，`P/` 在本节专指计划的 `backend/nyanpasu-rpc-protocol/src/`（不是历史 platform），`H/` = 计划的 `backend/nyanpasu-server/src/`，`L/` = 计划的 `backend/nyanpasu-cli/src/`。POC 一律通过 `git show 4a4a3a0d3:<path>` 读取；冒号后的行号属于该 revision，不能与 HEAD 混用。不整支 cherry-pick。
- Rust 命令从 `backend/` 执行；脚本命令从根目录执行。每卡执行 §0.7/§0.8；清单检查 cargo 成功、非空、保留 stderr、排序后比较，只允许登记的模块前缀变化，新增测试单列。新测试默认 unit target；只有 E4d 新增 `tests/session.rs`。本文命令供后续实施，本次文档修改不运行 Rust 编译/测试。
- **接线约定 G：** 建包同提交更新 `backend/Cargo.toml` members/消费者 manifest、`S/backend-boundaries.ts:28,34,42` 集合/规则及 `S/backend-boundaries_test.ts` 正反例、`S/policy.ts:81` 的 `STATIC_GATE_PREFIXES` 与 `S/architecture-ledger_test.ts:996` 覆盖矩阵、musl runner `scripts/src/testing/test-backend-musl.ts:68,77` 的 `-p` 与 `expectedTargets` 两清单。检查 resolved normal/build、all-features/all-targets 的直接及传递边；dev-only 测试装配另验，不误禁合法 dev 边。指向 core/config 的每条边 `default-features = false`，显式转发所需 feature；不得用全 feature 通过掩盖单包 feature 泄漏。无新包的卡明确保留/调整 G，不能漏掉 feature 或 target 注册。
- **接线约定 B：** 每卡重导出 `T/specta_export.rs:486` 的三份 bindings（路径 `:448,452,456`），相对该卡父提交零 diff，包括 D/A2b 已合法新增的类型；不得回退到本次旧基线。现有命令名、camelCase 参数、query/mutation 分类、错误字段、十一事件名/载荷不变。新增 local 帧不进入既有前端 schema；本轮 E 无页面、组件、i18n 或前端传输修改例外。
- **接线约定 V：** 每卡跑所列 Rust 测试、§0.7 两门禁/Clippy、`pnpm run test:scripts`、`pnpm run test:architecture-ledger`；新包/feature/target 跑 `pnpm run test:backend-musl`，核实发现并执行目标。Windows 本地可用时执行，否则 Windows CI；Linux 行为测试与 GNU 检查交 Linux CI，macOS 分支交 macOS CI；跨编译不替代平台行为测试。musl 只验证 Linux 中立目标，不声称覆盖命名管道或 OpenWrt SDK/硬件。`rg` 限定 crate 与 `--glob '*.rs'`，1 = 无匹配、2 = 检查失败；另审 grouped/relative/aliased imports。
- 每卡更新本卡、design §5.4/里程碑与路线图实施状态；建包卡同步 backend-packages/architecture，registry/transport/event/audit 卡同步 rpc 指南。文档中的计划不能提前写成已交付。G/B/V 在每卡验证中引用，均是必做项。

| PR    | 卡                            | 前置/合入顺序             | 内容                                                        |
| ----- | ----------------------------- | ------------------------- | ----------------------------------------------------------- |
| PR-E1 | E1a-a → E1a-b → E1a-c → E1a-d | PR-A2b（D2 → D3 → D4 后） | 错误接缝、protocol 纯移动、宏接缝、能力 admission 行为      |
| PR-E2 | E2a → E2b → E2c               | PR-E1、C2f                | 共享命令接缝、registry/audit/HTTP 纯移动、bus-first 接缝    |
| PR-E3 | E3a → E3b → E3c → E3d → E3e   | PR-E2                     | API-only；帧/协商、端点、连接资源、client 与桌面 local 接入 |
| PR-E4 | E1b-a                         | PR-E3（含 E3e）、D2e      | 完整 DTO 闭包及显式接收投影                                 |
| PR-E5 | E1b-b                         | PR-E4、E3e                | typed client 闭包与全部命令/schema 验收                     |
| PR-E6 | E4a → E4b                     | PR-E5、C2d、C3a           | 首次装配即拒绝提权；长期进程/listener 生命周期              |
| PR-E7 | E4c                           | PR-E6                     | 小型 CLI，仅连接既有 shell                                  |
| PR-E8 | E4d                           | PR-E7                     | 独立 CLI 进程共享会话/断连/owner 隔离集成验收               |

---

## E1a-a — 错误转换留在注册表消费侧

本卡是保行为接缝；不移动类型，不改错误分类、HTTP status、Display 或 domain payload。

- [ ] 将 `T/unified_rpc.rs:290,294` 的 `RpcError::application`/`application_ref` 改为本模块自由函数；宏 `M/unified_command.rs:300`、HTTP 路由 `T/unified_rpc.rs:475,525,667` 与全部调用者同改。`RpcError` 仍在原文件，为下一卡跨包后不能补 inherent impl/orphan From 做准备。
- [ ] 保留 LogError、observers `CoreLogError`、core-manager `CoreError`、`T/ipc.rs:55` `IpcError` 的逐项 downcast，以及 code/retryable/operation_id/domain_error。`IpcErrorKind::DebugHttp`（`:77`）不能被 POC 旧映射覆盖；Tauri error 转换（`:164`）暂留 shell。
- [ ] 同卡把 `RpcOwner::http/http_or_anonymous`（`T/unified_rpc.rs:225,242`）的 HeaderMap/cookie 解析与 `RpcError::status:334` 改 shell 自由函数，所有 HTTP 调用点直接迁；协议值只留纯构造/label/字段。HTTP UUID owner、无 cookie 匿名行为、status 映射保持原样，下一卡不再切逻辑。
- [ ] 参考 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/error.rs` 的 `application_error_ref:159`，只取自由函数形状；丢弃旧 core/log/OS 路径。`RpcError::status`（HEAD `:334`）仍属 HTTP adapter，不随 envelope 下移。
- [ ] 无新包/feature；G 的集合、prefix、musl 两表不变，既有策略正反例继续运行。更新 rpc 指南的转换 owner，不改传输语义。

**验证**

- `cargo test -p clash-nyanpasu errors_preserve_domain_codes_and_operation_identity`（`T/unified_rpc.rs:1354`）、`audited_failure_preserves_the_original_rpc_error`（`audit_tests.rs:425`）；按 §0.8 原测试清单无增删，执行 B/V。
- `rg 'RpcError::application(_ref)?' backend/tauri/src backend/nyanpasu-macro/src --glob '*.rs'` 退出码 1；正向审阅四类 downcast。反向删 code/operation_id 或把 domain_error 转字符串，上述测试必须失败；不以重写期望值掩盖差异。

## E1a-b — 中立协议基础定义

本卡是纯移动；不启用 local、不改变 owner 字符串或已知错误形状。

- [ ] 新建 `nyanpasu-rpc-protocol`；移动 `T/unified_rpc.rs:219,260,273,345` 的 owner 值、错误信封、JSON 值和请求信封到 P；HTTP cookie 解析（`:225`）、HTTP status 和 E1a-a 转换留消费侧。将 `EVENT_NAMES`（`:173`）的字符串定义归入 protocol，Tauri wrapper 仍在 shell，测试逐项与 `tauri_specta::Event::NAME` 相等。
- [ ] 迁 T/unified_rpc、ipc、specta_export、宏的协议类型引用与测试，不留旧 re-export。采用 `git show 4a4a3a0d3:backend/nyanpasu-rpc-protocol/src/event.rs` 的 `EventName:7`/`as_str:36` 及 `error.rs` 的信封分层；不带其新增 internal 错误语义、local owner 或 HTTP policy。
- [ ] G：workspace 加包；tauri 普通依赖 protocol，serde/json/specta 等依赖按实际类型闭包保留。protocol 属独立中立层，禁 GUI、全部 CORE_FAMILY（含 state-engine）、IMPLEMENTATION；foundation/core/implementation 禁上行 RPC adapter 的规则在 E2b 建包时注册。本卡 protocol 尚不依赖 core 或 D2e 值包。
- [ ] 策略正例 protocol → serde/config 值，负例 protocol → core/state-engine/observers/updater/OS/Tauri（含传递）；静态 prefix 加 `backend/nyanpasu-rpc-protocol/src/`，删除 prefix 的覆盖负例。musl 加 `-p nyanpasu-rpc-protocol`、`lib:nyanpasu_rpc_protocol`。同步 backend-packages/architecture/路线图。

**验证**

- `cargo test -p nyanpasu-rpc-protocol`、`cargo test -p clash-nyanpasu`；§0.8 两侧清单合计保留原测试，新增（新）`protocol_event_names_match_desktop_names`、`protocol_envelope_preserves_existing_wire_shape`；B/V。
- `rg 'tauri|nyanpasu_core::|nyanpasu_observers|nyanpasu_updater|downcast_ref|HeaderMap' backend/nyanpasu-rpc-protocol/src --glob '*.rs'` 退出码 1；T 中旧定义消失，cookie/status/conversion 正向保留。反向给 protocol 加 core/OS 依赖或改变事件字符串，gate/对应测试失败。

## E1a-c — 中立声明与 native wrapper 分离

本卡是保行为接缝；宏产生的内部结构改变，现有两传输暴露集合、参数与结果不变。

- [ ] 分离 `M/unified_command.rs:49,90,176,274`：中立声明只生成中立 schema/handler；native wrapper 独立做 `State`/AppHandle/Window/Webview/Channel 提取。共享 handler 的 dependency/caller 接口先定义在 T/unified_rpc，E2b 原样迁 R；不新增可配置 registry-path 兼容模式。
- [ ] 新增目标/能力元数据定义与 `rpc(local)`、required/optional 能力语法，但本卡所有生产命令仍只维持既有目标；admission/生产标注留 E1a-d，真实 local opt-in 留 E3e。命令名/目标/能力从声明生成单一 metadata 来源，不手写第二份名单；P/command_catalog.rs 是由窄范围生产声明 metadata extractor 生成并提交的中立 Rust artifact，保留 cfg guards，覆盖未链接 native handler 的已知名字。新增（新）`export_rpc_command_catalog` 显式生成，`rpc_command_catalog_is_fresh` 只比较不写；开发期读取声明不形成 protocol 的 normal/build 上行依赖。E1a-d/E3e 改标注时同提交重生成；既有三份前端 bindings 仍零 diff。registry 先从 catalog 建全部已知 metadata，再关联实际 handler；没有 native handler 视为缺能力 unsupported，不能因 headless 未链接 Tauri inventory 就丢名字。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-macro/src/unified_command.rs` 的 native 判断 `:79`、中立 handler `:361`、parser `:435`；保留 HEAD parser `:310` 的 camelCase、零参/null、未知参数兼容。不移入 ticket 注入，也不把 POC 固定 registry 路径当兼容 shim。
- [ ] Tauri channel reconstruction 保留（HEAD `:116`）；`rpc(owner)`/Result alias 保留。`T/specta_export.rs` 仍收集完整 schema、transport builder 仍只注册 call_rpc；完整 command inventory 包含 cfg 分支，不把 163 声明当单平台同时注册数。
- [ ] G 无新包，protocol 新 metadata 值无 core 边；宏依赖保持 proc-macro 工具依赖，不链接应用 runtime。prefix/musl 两表不增；同步 rpc 指南的计划/当前状态。

- [ ] extractor 只遍历显式列出的生产声明根及其 inline/external `mod`，继承祖先 `cfg`/`cfg_attr` 条件，排除 `cfg(test)` 与测试 fixture；不扩展为任意 Rust 模块解析框架。HEAD `M/unified_command.rs:593,639` 只读 T/ipc.rs，`:752,764` 丢祖先属性，不能直接复用；Windows 模块 `T/ipc.rs:1446` 的条件必须保留。E2a 根改为 T/ipc.rs + T/rpc_commands，E2b 改为 T/ipc.rs + R/commands；声明根、catalog freshness、schema/handler 集合同时迁移。
- [ ] dispatcher 覆盖按类别断言：native 声明有 native dispatcher；shared 声明有中立 handler 与其 opt-in transport dispatch；catalog 的未链接 native 名保留 unsupported metadata。原 `every_ipc_rpc_command_has_a_desktop_dispatcher:592` 改为这些分类断言，§0.8 逐项登记替换，不原样套用到中立命令。新增（新）`catalog_preserves_mutually_exclusive_platform_cfg`、`catalog_follows_external_shared_modules`、`catalog_excludes_test_fixtures`、`dispatchers_cover_each_command_category`；反向丢祖先 cfg、外部模块或引入 fixture，测试失败。
- [ ] 明确丢弃 POC `git show 4a4a3a0d3:backend/nyanpasu-macro/src/unified_command.rs:101-110` 拒绝 `rpc(local)` + optional elevation 的检查；optional reachable elevation 不否定合法 local 声明。新增（新）`local_optional_elevation_expands_and_registers`（宏展开与 registry admission 两侧，E3e 用已审 profile mutation 固定）；required 能力仍按 E1a-d 判定，server 的拒绝由 E4 注入策略保证。

**验证**

- `cargo test -p nyanpasu-macro`、`cargo test -p clash-nyanpasu`；保留 `explicit_result_alias_is_unwrapped_and_owner_is_injected:443`、`channel_parameters_are_rebuilt_for_desktop_but_unsupported_over_http:483`、`http_requires_opt_in_and_accepts_only_lifetime_generics:514`、`desktop_update_core_dispatch_remains_available:552` 与 inventory tests `:638,688`；`:592` 按下述类别测试替换，路径均 M/unified_command.rs。新增（新）`neutral_expansion_contains_no_tauri_types`、`target_and_capability_metadata_is_generated_once`、上述 catalog 生成/只读 freshness 两测试；§0.8/B/V。
- `rg -n 'AppHandle|JavaScriptChannelId|tauri_handler|neutral_handler' backend/nyanpasu-macro/src/unified_command.rs` 是正向分支审查，不要求整个宏无 Tauri token；反向让 neutral 分支生成 native handler，新增展开测试失败。protocol 中 Tauri 搜索仍零匹配。

## E1a-d — 按目标与能力拒绝 admission

本卡是行为变更：缺少 required 能力的 shell 对已知命令（含未链接 native handler）返回结构化 `unsupported`，不执行 handler、不伪装成 `unknown_method`；现有桌面/HTTP 行为保持不变。

- [ ] 按 design §5.4 的能力表逐项标注 T/ipc.rs；caller identity 与 native Channel/Webview 分开；required 与 optional/reachable effects 分开，不能因某字段可能需要展示就拒绝所有 config patch。标注不是 per-call authority；不改 O-5。
- [ ] `CommandEntry`（HEAD `T/unified_rpc.rs:251`）保存目标与能力元数据，registry 先辨识已知命令，再检目标/required capabilities，最后解析/调用；未知命令才 unknown_method。拒绝经过 `dispatch_audited:142`；保留 HTTP unsupported/501 和 `update_core` admission 前拒绝。全部重复 handler 名在能力判定前检查；native handler 未链接也保留 E1a-c catalog metadata。
- [ ] 参考 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/registry.rs` 的 metadata `:38`、target dispatch `:186`；丢弃 `new:137` 对缺能力条目的删除及测试 `commands_needing_a_missing_capability_are_not_registered:330`。不以 POC 的空 required-capability inventory 代替有效 registry。
- [ ] G 无新包；构造根显式传当前桌面可用能力，无隐藏探测或 global。prefix/musl 两表不变；补宏与 registry policy 正反测试，更新 rpc 指南 admission 规则。

**验证**

- 新增（新）`known_unavailable_command_is_unsupported_before_handler_runs`、`unknown_command_remains_unknown_method`、`optional_effect_metadata_does_not_reject_unrelated_config_edits`、`duplicate_commands_fail_even_when_a_capability_is_missing`、`headless_registry_knows_unlinked_native_commands`。§0.8 原清单加五例，B/V；保留 `http_update_core_is_unsupported_before_job_admission`（T/unified_rpc.rs:708）与 `audit_records_rejected_dispatches`（audit_tests.rs:286）。
- `rg -n 'unsupported|unknown_method|capabilit|dispatch_audited' backend/tauri/src/unified_rpc.rs` 审 admission 顺序；反向删除已知条目、先调用 handler 或不审计拒绝，新增/既有测试失败。无旧路径移动，沿用 E1a-b 协议依赖反向检查。

---

## E2a — 共享命令与特殊路由的中立接缝

本卡是保行为接缝；先在 Tauri crate 内切开 native/shared，下一卡只移已中立的模块。

- [ ] 将 T/ipc.rs 的共享 profile/config/runtime/Clash API/storage/log/traffic/status thin commands 分组到临时规范位置 `T/rpc_commands/`，调用方直接迁移，不在 ipc 留 re-export；只替换 Tauri State/Window 为 E1a-c 的显式依赖/caller。原生命令仍在 ipc；`subscribe_clash_connection_details:1951` 的 Channel 不伪装为 JSON 共享参数。`specta_export` 使用下述 schema-only collector 收集新位置。
- [ ] `IpcError`/映射（T/ipc.rs:55,67,128）进入中立命令模块；`tauri::Error` 的旧自动转换改 shell 调用点显式 `map_err`，保留 unknown 的 Display/detail，不让新错误模块依赖 Tauri。引用 D2e 已落地的 runtime-values，不恢复 core 旧值路径。
- [ ] 拆 `T/unified_rpc.rs:249,450,575` 的 native handler registry/IPC binding 与 shared registry；native fallback 和 shared handler 均只进入一次 C2f kernel。`RpcHttpRoutes:362` 保留 weak/实例 OnceLock 语义；不形成 client/router 强引用环。
- [ ] `T/utils/collect.rs:211` 的 archive-only 编排及必要 `write_core` 辅助分离为中立模块，供 HTTP `T/unified_rpc.rs:475` 与 native save-dialog `T/ipc.rs:1124` 共用；报告窗口、webview 信息、crash reporter 不跟随。HTTP special-route admission 仍审计，SSE 生命周期不塞入 kernel。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/commands.rs` 的分组 `:17`、`registry.rs` 的 `State:61`/`Caller:81` 与 `backend/tauri/src/unified_rpc.rs` 的 DesktopRpc；不直接照搬 POC 的 HTTP exposure，不迁 lifecycle 新命令/tickets。现有每个 HTTP 命令必须有迁/留清单，留 shell 的 host 命令有明确能力原因。
- [ ] 无新 crate；G 不增加集合/prefix/musl target，tauri manifest 清理仅本卡产生的未用项；core/config defaults-off 不变。更新 rpc 指南的 schema/native binding 入口。

- [ ] 在 `T/specta_export.rs:34` 将 schema 收集改为只收 Specta functions 的 `schema_commands!`，适配 `git show 4a4a3a0d3:backend/tauri/src/specta_export.rs:36`；不能调用会生成 `tauri::generate_handler!` 的 `tauri_specta::collect_commands!` 来收中立函数。`build_transport_builder` 仍只注册 `call_rpc`（宏测试 `M/unified_command.rs:688`）；三份 B 生成文件逐字节不变。新增（新）`schema_collector_accepts_neutral_external_commands`、`full_http_exposure_is_preserved_across_shared_split`，后者按平台/cfg 比较拆分前后完整集合，POC 为 85 个 extracted RPC HTTP 声明加 20 个 retained shell 声明，共 105 个不同名字，不能替代 HEAD 104 项基线。
- [ ] 下表穷尽 HEAD 104 个 HTTP 声明；共享入口迁 T/rpc_commands 后随 E2b 迁 R/commands。留 shell 的实现通过 RPC 消费侧定义的窄 port 注入：shared HTTP entry 仍注册在同一 registry 并进入一次 audit，不留仅 native registry 能调用的 HTTP handler。端口缺失只使已知命令 unsupported。E2a/E2b 同步迁 extractor 根，验证外部模块声明未漏。纯文件/icon 查询不是 presentation capability；不要因无窗口而禁用查询。

POC exposure 差异：`git show 4a4a3a0d3:backend/nyanpasu-rpc/src/commands.rs`（:41 起及 production 子模块）共 85 个 HTTP 声明（root 13、子模块 72），`backend/tauri/src/ipc.rs`（:236–1154）保留 20 个，共 105 个不同名字。HEAD 独有 `get_ip_lookup_providers`（T/ipc.rs:849）；POC 额外暴露 `get_core_version` 与 HTTP `update_core`（POC T/ipc.rs:520）。E 不恢复后者 HTTP 权限。

HTTP 迁/留处置（锚点均为 `T/ipc.rs` 的函数；共 104 声明，无删除/新增 HTTP opt-in）：

| 声明                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 迁移/注入                                                                     | 语义约束                                                                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `get_profiles`（:392）、`is_portable`（:399）、`enhance_profiles`（:408）、`import_profile`（:433）、`create_profile`（:494）、`reorder_profile`（:506）、`reorder_profiles_by_list`（:517）、`set_profile_layout`（:527）、`set_proxy_group_layout`（:537）、`create_proxy_group_folder`（:548）、`create_profile_folder`（:562）、`get_profile_sync_status`（:573）、`get_profile_sync_runs`（:583）、`get_profile_sync_logs`（:594）、`update_profile`（:606）、`delete_profile`（:617）、`activate_profile`（:627）、`set_global_transforms`（:637）、`set_profile_valid_fields`（:647）、`patch_profile_metadata`（:657）、`patch_remote_profile_options`（:668）、`replace_profile_definition`（:679）、`read_profile_file`（:702）、`save_profile_file`（:712） | 迁 shared profiles / 路径值 组；原 client/path 依赖显式注入                   | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_clash_info`（:723）、`get_runtime_config`（:735）、`get_runtime_yaml`（:747）、`inspect_runtime`（:754）、`inspect_applied_runtime`（:763）、`inspect_runtime_node`（:772）、`get_runtime_exists`（:783）、`get_postprocessing_output`（:795）、`get_core_status`（:809）、`probe_direct_egress`（:842）、`get_ip_lookup_providers`（:849）、`get_app_config`（:882）、`patch_app_config`（:889）、`get_clash_config`（:899）、`get_config_features`（:906）、`patch_clash_config`（:913）、`patch_runtime_overrides`（:925）、`get_hotkey_functions`（:958）、`change_clash_core`（:968）、`restart_sidecar`（:977）、`get_sys_proxy`（:987）、`get_core_agent_skills`（:1079）                                                                                  | 迁 shared runtime / config / core / host 状态 组；原 client/path 依赖显式注入 | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_core_dir`（:1065）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | 保留 executable resolver 实现，注入 CoreDirectoryResolver（新）               | 沿用 `tauri::utils::platform::current_exe`（:1066）、parent/canonicalize/error；禁静默换 std::env::current_exe |
| `fetch_latest_core_versions`（:1105）、`inspect_updater`（:1162）、`clash_api_get_proxy_delay`（:1172）、`clash_api_get_configs`（:1185）、`clash_api_delete_connections`（:1194）、`clash_api_get_version`（:1204）、`clash_api_get_rules`（:1213）、`clash_api_get_providers_rules`（:1222）、`clash_api_update_providers_rules`（:1231）、`clash_api_get_group_delay`（:1241）、`clash_api_get_providers_proxies`（:1253）、`get_proxies`（:1263）、`mutate_proxies`（:1272）、`select_proxy`（:1281）、`clear_proxy_fixed`（:1292）、`update_proxy_provider`（:1302）、`clash_api_healthcheck_proxy_provider`（:1309）                                                                                                                                             | 迁 shared updater / Clash API / 平台值 组；原 client/path 依赖显式注入        | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_cached_icon`（:1421）、`status_service`（:1597）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 迁 shared cached icon / service status 组；原 client/path 依赖显式注入        | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_tray_icon`（:1431）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | 保留 icon 实现，注入 HostIconFiles（新）                                      | 纯读取 PNG，不需要展示能力                                                                                     |
| `set_tray_icon_from_bytes`（:1489）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | 文件写入注入 HostIconFiles；refresh 仍调用 client                             | 保留解码/1 MiB/校验错误与 presentation refresh 顺序                                                            |
| `is_tray_icon_set`（:1505）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 保留 icon 实现，注入 HostIconFiles（新）                                      | 纯文件查询，不需要展示能力                                                                                     |
| `get_widget_icon`（:1531）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | 保留 icon 实现，注入 HostIconFiles（新）                                      | 纯读取 PNG，不需要展示能力                                                                                     |
| `is_widget_icon_set`（:1541）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | 保留 icon 实现，注入 HostIconFiles（新）                                      | 纯文件查询，不需要展示能力                                                                                     |
| `set_widget_icon_from_bytes`（:1564）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 文件写入注入 HostIconFiles；refresh 仍调用 client                             | 保留解码/1 MiB 与 presentation refresh 顺序                                                                    |
| `get_service_install_prompt`（:1666）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 保留 OS 参数解析，注入 ServiceInstallPrompt（新）                             | 只生成文字，不提权；保留平台 quoting/error，R 不依赖 OS                                                        |
| `get_storage_item`（:1714）、`set_storage_item`（:1721）、`remove_storage_item`（:1732）、`get_hotkeys`（:1739）、`set_hotkeys`（:1746）、`get_all_storage_items`（:1772）、`clear_storage`（:1792）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 迁 shared storage / hotkeys 组；原 client/path 依赖显式注入                   | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_clash_ws_snapshot`（:1808）、`query_core_logs`（:1817）、`get_core_log`（:1827）、`get_core_log_status`（:1837）、`clear_core_logs`（:1846）、`get_traffic_summary`（:1853）、`query_traffic_report`（:1862）、`query_traffic_usage`（:1872）、`query_traffic_usage_by_keys`（:1888）、`query_traffic_closed_connections`（:1902）、`query_traffic_active_connection_ids`（:1917）、`set_clash_ws_recording`（:1927）、`clear_clash_ws_history`（:1938）                                                                                                                                                                                                                                                                                                          | 迁 shared logs / traffic / streams 组；原 client/path 依赖显式注入            | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `list_log_files`（:1990）、`open_log_session`（:1999）、`query_logs`（:2012）、`close_log_session`（:2025）、`report_frontend_events`（:2038）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | 迁 shared log sessions / frontend events 组；原 client/path 依赖显式注入      | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `get_system_accent_color`（:2227）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 保留 host 查询，注入 AccentColorQuery（新）                                   | 只查颜色，不等同 native webview capability；保留 Option                                                        |
| `get_configuration_status`（:2239）、`retry_configuration_runtime`（:2247）、`retry_configuration_effect`（:2253）、`get_debug_http_status`（:2561）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 迁 shared configuration / debug HTTP 组；原 client/path 依赖显式注入          | 结果、错误、HTTP exposure 不变；可选效果按既有 client 能力判断                                                 |
| `is_appimage`（:1339）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 注入 HostEnvironment 值（新），保留 shell 对 `IS_APPIMAGE` 的计算             | 不把 shell const/global 引入 R；相同进程结果不变，无 presentation 要求                                         |

- [ ] 在定义 host ports 前先闭合最小 shared signature：`GetSysProxyResponse`（T/ipc.rs:369）、`IconData`（:1414）、`service::ServiceStatusInfo`（:1584）、`StorageEntry`（:1761）随对应命令移到 T/rpc_commands 的中立类型模块，E2b 一起移 R；这四者已供 HTTP 使用，不是 native-only DTO。保留字段、serde/Specta 名称与内层 core 类型，完整向下投影留 E1b；不让共享签名引用 T/ipc 或 tray 模块。
- [ ] `TrayIcon`（T/core/tray/icon.rs:12）作为 shared wire mode 先移 P 的中立值模块；`Normal/Tun/SystemProxy`、snake_case、Display/From、`as_str`/`all_supported` 等纯值实现同迁。`:59,60` 的 asset `raw_bytes` 改 T 的 free function，`get_raw_icon:82` 与 tray/widget/native IPC 等所有调用者直接更新；禁止在 shell 对外部 `TrayIcon` 写 inherent impl，不留 re-export。HostIconFiles 签名只引用 P/TrayIcon；基础包不依赖 protocol。
- [ ] G/B/V：协议已有包/prefix/musl 两清单不变；tauri 直接引用 protocol mode，E2b 的 R 通过已有 protocol 边消费，core/config defaults-off 保留。新增（新）`shared_signatures_have_no_shell_owned_types`、`tray_icon_wire_mode_matches_existing_schema`；原 shell icon 测试按 §0.8 跟随 free function 调用迁移，不删案例。scoped `rg 'crate::ipc::(GetSysProxyResponse|IconData|StorageEntry)|crate::ipc::service::ServiceStatusInfo|crate::core::tray::icon::TrayIcon'` 检查 T/rpc_commands 与迁后 R 零匹配；另审 alias/group imports。反向恢复 shell 类型边或 foreign inherent impl，依赖/编译与 bindings 检查失败。

**验证**

- `cargo test -p clash-nyanpasu`、`cargo test -p nyanpasu-macro`；§0.8 原清单不减，含 `shared_profile_command_over_experimental_http:813`、`errors_preserve_domain_codes_and_operation_identity:1354`、`connection_details_send_current_and_new_frames_and_release_demand:1380`（T/unified_rpc.rs），audit_tests 全部保留；B/V。
- `rg 'tauri::|AppHandle|Webview|JavaScriptChannelId' backend/tauri/src/rpc_commands --glob '*.rs'` 退出码 1；按迁移清单检查 ipc 旧函数/旧 imports，无 re-export。反向丢 owner、把 native 调用绕过 audit、archive 带回 reporter，各对应 owner/audit/依赖审查失败；不只靠整个 crate 可编译。

## E2b — 注册表、审计与 HTTP 中立模块同迁

本卡是纯移动；保留 Tauri → bus 的旧输入桥到 E2c，保留无 assets 不能启用到 E3a。

- [ ] 新建 `nyanpasu-rpc`，迁 E2a shared registry/dependencies/commands/error/archive 及 `T/server/debug_http.rs` 中立 actor/router/assets port，按职责放 R/registry.rs、commands/、error.rs、http/。`T/unified_rpc.rs:42,80,111,142` 的 RpcAudit/sink/guard/kernel 同迁；T/adapters/rpc_audit.rs:5,20 的 Tauri-free `TracingRpcAuditSink` 同迁 R，两个 shell 注入并复用同一个中立生产实现，保留 DEBUG/INFO 分级与 secret exclusion。不能另造一个 local audit 实现。
- [ ] 迁 EventBus（T/unified_rpc.rs:188），暂由 shell 的 `bridge_tauri_events:205` 调新 bus。native entry/call_rpc、Tauri event derives、`T/setup.rs:832` 的 AssetResolver 留 shell；T/setup.rs:615 的 registry/routes 装配、lib 模块注册、宏生成路径、Specta、所有测试 import 同提交迁完。删除 T/server 的旧中立模块与 T/rpc_commands，不留旧路径转发。
- [ ] HEAD application_error 的 LogError/observers/CoreError/IpcError 映射随迁；可复用 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/registry.rs` 的 WeakUnifiedRpc `:111`，不替换 C2f kernel；POC `registry.rs:186` 的无审计直调和 shell archive 无审计路径丢弃。
- [ ] G：RPC 是 core 上方的应用适配层，依赖 protocol/core（defaults off）、实际消费的其他中立包；禁 GUI/IMPLEMENTATION。对 FOUNDATION、CORE_FAMILY（含 engine）、IMPLEMENTATION 逐项新增禁止依赖 rpc；protocol 继续禁 core。正例 rpc → core → foundation、tauri → rpc；负例 rpc → OS/net/Tauri、core/values/engine/各实现 → rpc，含传递路径。D2e 已完成；现有 `nyanpasu-runtime-values` 无条件纳入实际 tree roots、FOUNDATION 规则与 values → rpc 禁止边的正反 fixtures，不保留条件接线。
- [ ] HTTP 依赖放 `http` feature，默认不隐式启用；tauri 显式开 http，保持原 feature/TLS。core/config 边均 defaults-off、转发 default-meta/nesting 等实际 schema feature。静态 prefix 加 `backend/nyanpasu-rpc/src/`；musl 加 `-p nyanpasu-rpc` 与 `lib:nyanpasu_rpc`，构建参数显式启用 rpc/http；不靠 tauri feature unification。
- [ ] 中立测试在 R 用 Tokio 与注入 fixture；native audit/browser/Channel 测试留 T，依赖 test-support 按 §0.5 开，不能从 rpc 测试依赖 Tauri。同步四份 development/路线图相关归属。

**验证**

- `cargo test -p nyanpasu-rpc --features http`、`cargo test -p clash-nyanpasu`、`cargo test -p nyanpasu-macro`；§0.8 两侧合计保持 E2a 清单，逐项记录 audit_tests 的迁/留：`tauri_dispatch_records_one_audit_event:146` 留 T，HTTP `:221`、special routes `:242`、secret exclusion `:376`、error preservation `:425`、level `:460`、drop `:515`、panic payload `:567` 迁 R 的中立 fixture；其中 secret exclusion/level 仍调用实际 `TracingRpcAuditSink`，原测试原样迁移（`audit_tests.rs:22,38,487,500`），不得换测试副本；E3d 在同一生产 sink 增 Local INFO 覆盖。B/V。
- `rg 'tauri::|tauri_specta|AppHandle|crate::ipc|crate::utils::collect' backend/nyanpasu-rpc/src --glob '*.rs'` 退出码 1；T 中 scoped 搜旧 `server::debug_http`/rpc_commands 与已迁 kernel 定义，须消失。反向 rpc 加 tauri/OS、core 加 rpc、删新 prefix 或 musl target，分别 gate/覆盖/目标检查失败。移动后无 assets 仍拒绝，证明未混 E3a。

## E2c — 观测先发布到总线

本卡是保行为接缝；共享事件的名字、载荷、每源顺序与 lag/resync 不变，headless 可观测性由同一发布路径提供。

- [ ] 将 `T/setup.rs:334,776` 的 storage/configuration/core/proxy/service forwarders、`T/adapters/clash_events.rs:21` 的 logs/ws 发布接到 R/forward.rs；C/client/event_sink.rs:28 的 UiEventSink 用 bus adapter，app-update sink（T/adapters/app_update.rs:242）同理。client 构造（T/setup.rs:368）已经启动 owners，不改变 pre-owner-start 构造。forwarder 构造同步获取订阅（:783,801,816,823），在 startup reconciliation、stream activation、listener admission 前完成；需要时获取初始 snapshot，root tasks/shutdown 显式拥有 forwarders。
- [ ] bus 拥有注入的同步 native delivery sink（Tauri adapter），publish 返回前完成 native delivery，不增加 native 异步队列；HTTP SSE/local 才用有界异步 receiver；删除已迁事件的 `listen_any` bridge（原 T/unified_rpc.rs:205），deep-link/window-message 等 shell-originated 事件显式 publish，不走回声桥。native Channel 与 `/bridge/connection-details` 仍是 demand stream，完整详情不进全局广播。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/events.rs` 的 publish/subscribe `:45,56` 与 `forward.rs:59` 的七源分组；更新旧 logs/storage 路径。native 同步 sink 保留现有 emit 时点且不得重入发布者；SSE/local 用现有有界 broadcast，慢消费者 lag 后 resync，不加无界队列。保留每源顺序，不承诺跨源因果全序。前端 native `listenResync` 无恢复实现，不能声称 native 异步丢失可 resync。
- [ ] G 无新包/feature/target；静态 prefix/musl 两表保留；新增 bus 正例（无 shell 发布）/负例（回声、lag 丢失）测试，更新 architecture/rpc/路线图的 bus-first 状态。

- [ ] 内部 delivery envelope 携带 single-window / window-type / all-windows target，不改变 wire payload。保留 `T/window/mod.rs:257,264,286,302` 的目标验证、`emit_to`/全局 emit 与不存在窗口的失败；丢弃 POC `git show 4a4a3a0d3:backend/tauri/src/unified_rpc.rs:79` 无条件全局 emit。新增（新）`native_delivery_preserves_single_window_target`、`native_delivery_preserves_window_type_target`、`native_delivery_preserves_all_windows_payload`、`native_delivery_rejects_missing_window`。
- [ ] 新增（新）`tauri_delivery_survives_async_subscriber_overflow`：实际 Tauri delivery adapter 边界的可注入 emitter 记录每次同步交付，塞满 SSE/local receiver，断言 native 无丢失、无延迟队列且异步侧收到 lag/resync；不能只测试 broadcast receiver。forwarder 新测试在构造后、首次 poll 前发布，断言事件不丢且 snapshot/stream 激活无缺口；不新增前端修改。

**验证**

- 新增（新）`shared_events_publish_without_tauri`、`bus_adapters_do_not_echo_events`、`bus_preserves_source_order_and_reports_lag`、`forwarder_captures_publication_before_first_poll`。参考 POC `events.rs:80,104` 的测试意图，不把它们称为 HEAD 既有测试；§0.8 登记新增清单（含下述 adapter/target cases），B/V。
- 保留 `C/storage.rs:790` 的 `a_lag_becomes_a_resync_and_forwarding_continues_until_closed`、T/unified_rpc.rs:1380 的 details demand 测试；`rg 'bridge_tauri_events|listen_any' backend/nyanpasu-rpc/src backend/tauri/src/unified_rpc.rs --glob '*.rs'` 退出码 1，另逐源审 setup/adapters 不重复 emit。反向加回 bridge、把 queue 改无界或订阅延迟到 first poll，回声/lag/启动测试须失败。

---

## E3a — 无前端资源的 HTTP API

本卡是行为变更：未提供 assets/dev frontend 时也能启动 API；静态页面缺失返回正常 404，现有认证与 API 结果不变。

- [ ] 在 E2b 后 R/http 的启动分支去掉原 `T/server/debug_http.rs:138-141` 的必需 frontend，仅 Some 时挂 `frontend_router` fallback（原 `:151`）；同迁后的 HttpRoutes/actor 调用链一起改，不增加第二个 API server。
- [ ] 保留原 `local_session:271` 的 per-start credential、owner cookie、Host/Origin 检查、loopback 与默认 disabled；assets 可选不等于公开 LAN。参考 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/http/listener.rs` 的 `bind_loopback:34`/middleware `:87`；不引入 router_listener、WebTickets 或 ticket 登录。
- [ ] G 无新包，http feature/prefix/musl 两表保留；更新 rpc/路线图 API-only 状态。无前端源码/生成文件差异。

**验证**

- 原 `unavailable_frontend_keeps_server_disabled`（T/server/debug_http.rs:826）明确替换为（新）`api_starts_without_frontend_assets`，按 §0.8 登记唯一预期删除/新增；新增（新）`api_only_keeps_credentials_host_and_origin_checks`。保留原 `embedded_frontend_serves_assets_spa_and_real_404s:606`、`actor_serializes_start_stop_and_rejects_foreign_origins:662`、`stopping_closes_an_open_event_stream:788`、`dev_proxy_preserves_query_status_and_websocket_protocol:835`；迁后路径按符号定位，B/V。
- `rg 'HTTP frontend is unavailable|WebTickets|ticket=' backend/nyanpasu-rpc/src/http --glob '*.rs'` 退出码 1；反向恢复 None 拒绝/移除中间件，两个新测试失败。测试覆盖原 assets 模式，不只测空 router。

## E3b — local 帧与握手协议

本卡是行为变更（新增协议能力），尚不暴露生产 listener；协商成功前不接受命令。

- [ ] P/frame.rs 定义长度前缀 JSON request/response/error/event/subscription 帧、请求关联与显式 hello/accept/reject。适配 `git show 4a4a3a0d3:backend/nyanpasu-rpc-protocol/src/frame.rs` 的 `encode:98`/read `:131`/write `:162`；采用其大端 u32、4 MiB 上限作为初版，固定测试；不采用仅每请求比较 `v == 1` 代替协商。
- [ ] 初版协议 version 1；hello 提供支持版本/feature，server 选择共同版本与支持 feature，未知必需 feature 或无共同版本先拒绝且不 dispatch。IDs 为每连接严格递增正 u64 整数，完成后也永不复用，回复关联同 ID；订阅帧区分 request ID/subscription ID。JSON RpcError 内层保持既有字段，协议失败放 local 外层，不给 HTTP/Tauri 加 internal 变体。E1b 前的 local client 以拥有所有权的 JSON 值接收 result/error，不能要求 Deserialize 一个含 static str 或 skipped source 的生产错误；完整 typed receiving 在 E1b 完成。
- [ ] 超限/截断/非法帧终止连接，未协商或非递增 ID（包括已完成 ID 的复用） 不执行命令；明确 frame/session 状态机，用有界缓冲 codec 处理分片/粘包。错误模块用 snafu，不暴露任意请求内容到 audit。
- [ ] G：protocol 加无默认启用的 `frame-io`（可选 Tokio/codec 依赖），纯值层不要求 IO；server/client 后续分别启用。无新包/prefix；musl 仍同 lib target，但显式加入 protocol/frame-io 构建与测试；增加 feature-off 正例与引入 core/server 的负例。同步 rpc 协议说明。

- [ ] 初始 features 固定为 `rpc`（必需）、`events`、`connection-details`（可选）；server 选 hello 与 server 支持集合的最高共同 version（初版只有 1），features 返回必需集加可选交集，未知 optional 忽略，未知 required 拒绝。握手只允许一次；字符串字段 UTF-8，version/length 为 u32，request `id` 为正 u64 JSON 整数（拒浮点/溢出），subscription 为服务端生成、不复用的 opaque string；耗尽 ID 后新建连接，不能回绕。
- [ ] 规范 JSON 示例（每行一个长度前缀帧；RPC response 的 `result`/`error` 二选一，error 内层仍为既有 RpcError）：

```jsonl
{"type":"hello","versions":[1],"requiredFeatures":["rpc"],"optionalFeatures":["events","connection-details"]}
{"type":"accept","version":1,"features":["rpc","events","connection-details"]}
{"type":"request","id":1,"method":"get_profiles","params":null}
{"type":"response","id":1,"result":{"valid":[],"items":[]}}
{"type":"subscribe","id":2,"stream":"events","event":"core-logs-changed"}
{"type":"subscribed","id":2,"subscription":"s1"}
{"type":"reset","subscription":"s1","event":"core-logs-changed","snapshot":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"event","subscription":"s1","name":"core-logs-changed","payload":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"lag","subscription":"s1","reason":"overflow","resync":true}
{"type":"reset","subscription":"s1","event":"core-logs-changed","snapshot":{"status":{"generation":"g1","version":0,"first":null,"head":null,"bytes":0,"error":null,"discarded":0}}}
{"type":"unsubscribe","id":3,"subscription":"s1"}
{"type":"unsubscribed","id":3,"subscription":"s1"}
{"type":"reject","code":"unsupported_version"}
```

- [ ] 示例 event/reset 是完整 CoreLogStatus wrapper（`backend/nyanpasu-observers/src/logs/model.rs:129-136`），不代表日志行；生产保持 EVENT_NAMES 对应完整载荷。状态机：握手 → ready → subscribe pending → subscribed ack → active events；ack 前不得发 event。unsubscribe 停止生产、清理或丢弃排队事件后发 ack，ack 后该 ID 无 event；ack 前允许已排队 event，迟到完成走 E3d cleanup。重复/未知 subscription unsubscribe 回幂等 ack；未订阅事件流不得发送。lag notice 后必须先发下表的 reset，客户端仅凭 reset 重建该流当前状态，再继续 event；不能调用未 allowlist 的 query。reset 与 unsubscribe ack 同受有序 writer 约束；不承诺跨源全序。
- [ ] 坏 JSON/未知 frame type/非法字段类型/超长/截断/握手前 dispatch/非递增 ID：无命令执行，能安全写时发外层 protocol reject 后关连接，否则直接关；无共同版本/缺 required feature 同样 reject 并关。已协商的合法 RPC 业务错误仍关联 id 回复，不关闭连接。新增（新）`completed_request_ids_cannot_be_reused`、`unsubscribe_ack_prevents_late_events`、`subscription_ack_and_lag_order_are_preserved`、`malformed_frames_never_dispatch`；反向放行已完成 ID 或 ack 后 event，测试失败。

- [ ] `get_profiles` 示例为 `Profiles` 对象，`valid`/`items` 必需，未设置的 optional document fields 省略（`nyanpasu-config/src/profile/profiles.rs:12,22,32`）；新增（新）`canonical_local_examples_decode_as_real_command_results`，不能只把示例 parse 成任意 JSON 就算通过。
- [ ] 初版 `events` 每次 subscribe 必须给 `event`（EVENT_NAMES 字符串），每个 subscription 只恢复该流；没有“订阅全部并忽略未知流”的隐式接口。恢复契约如下，所有载荷沿既有 serde shape，E1b 再闭合 typed receiving；不增加十三命令 allowlist。reset 外层为 `{"type":"reset","subscription":"s1","event":"configuration-status-changed","snapshot":...}`，`snapshot` 必须是表中完整值，不是 `resync: true`。订阅 ack 后先发 initial reset，lag 后先 lag 再 fresh reset，之后才恢复完整值交付。

| 本地 events identity                                    | 完整值证据与 initial/reset snapshot                                                                                                                                     | E3 范围                                                                                    |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `configuration-status-changed`                          | T/setup.rs:797 每次读取完整 `ConfigurationStatus`；C/client/configuration_status.rs:63 `configuration_status()`                                                         | 保留；event/reset 均替换当前 configuration status                                          |
| `core-status-changed-event`                             | T/adapters/events.rs:25 的 `CoreStatusInfo`，T/setup.rs:807 完整投影；C/client/mod.rs:892 `core_status()` 同投影                                                        | 保留；event/reset 均替换当前 core status                                                   |
| `service-status-changed-event`                          | T/adapters/events.rs:28 的 `ServiceHostStatus`，T/setup.rs:826-827 克隆完整 watch 值；C/client/mod.rs:898 `service_status()`                                            | 保留；event/reset 均替换当前 service status                                                |
| `app-update-state-changed`                              | T/adapters/events.rs:20-22 transparent `AppUpdateSnapshot`；T/adapters/app_update.rs:242-244 发布完整 snapshot；C/client/mod.rs:739 `get_app_update_state()`            | 保留；能力存在才可订阅，缺能力 structured unsupported；event/reset 均替换当前 update state |
| `core-logs-changed`                                     | T/adapters/events.rs:31-33 的 `{"status": CoreLogStatus}`；T/adapters/clash_events.rs:33-35 克隆完整 watch status；C/client/clash_streams.rs:21 `get_core_log_status()` | 保留的仅是完整日志状态，不是日志行/历史；initial/reset 保留同一 status wrapper             |
| `clash-ws-event`                                        | T/adapters/events.rs:36-38；C/clash/ws.rs:157-164 混合 reset 与局部 update/history-cleared                                                                              | 排除；并非每个 event 都是完整当前值，不能沿用 snapshot-plus-delta 恢复                     |
| `state-changed-event`                                   | C/client/event_sink.rs:15 的 invalidation hints                                                                                                                         | 排除；恢复另经下述延期卡审议                                                               |
| `storage-value-changed-event`、`storage-resync-event`   | T/adapters/events.rs:54,59；T/setup.rs:341,349                                                                                                                          | 排除；单 key delta 与 source resync，不是完整 map；恢复另经下述延期卡审议                  |
| `scheme-request-received-event`、`window-message-event` | T/unified_rpc.rs:179,183 的瞬时请求/定向消息 identities                                                                                                                 | 排除；无可恢复完整当前状态，保留既有 shell 交付                                            |

- [ ] E3 仅为上表五个 full-value identities 构建 initial/reset snapshot；客户端逐 event 整体替换该流描述的值，不需 query 或 reducer 重建增量。reset 由 R subscription control 调现有 facade，十三命令 allowlist 不变。沿用同步订阅、有限缓冲和源内顺序；snapshot 期间的发布由既有启动/race 测试固定，不能在 reset 后回放更旧的排队值。持续溢出、读取失败、超过 4 MiB 上限仍沿已有 subscription_error/resource_limit 契约结束订阅，不静默截断或加无界队列。
- [ ] E3d/E3e 保留真实本地连接测试（新）`local_overflow_reset_reconstructs_each_recoverable_stream`，仅对上述五个 full-value identities 参数化：阻塞 writer 造成 local lag，恢复后只用 fresh reset 与后续完整 event 重建当前值，不发 recovery query。保留（新）`local_reset_preserves_updates_during_snapshot`；将拒绝测试限定为（新）`local_non_full_value_streams_are_rejected`，覆盖其余六个 identities，返回已有 unknown_stream 且不分配资源。不在 E3 实现/测试 storage reset、key mapping 或 state-invalidation recovery。

- [ ] connection-details 不在 EVENT_NAMES/bus 中，规范帧如下；`data.payload` 是 C/clash/ws.rs:52 `ClashConnectionDetails`（sequence + connections），空 connections 也是合法当前帧：

```jsonl
{"type":"subscribe","id":4,"stream":"connection-details"}
{"type":"subscribed","id":4,"subscription":"d1"}
{"type":"data","subscription":"d1","stream":"connection-details","payload":{"sequence":7,"connections":[]}}
{"type":"subscription_error","id":5,"code":"feature_not_negotiated"}
{"type":"subscription_error","id":6,"code":"unknown_stream"}
{"type":"subscription_error","id":7,"code":"resource_limit"}
{"type":"subscription_error","subscription":"d1","code":"snapshot_unavailable"}
```

- [ ] `events` 需要 negotiated `events` feature，details 需要 `connection-details`；未协商以关联 request id 的 feature_not_negotiated 拒绝且不分配资源；未知 stream/event（含初版排除的 identity）返回 unknown_stream，表满返回 resource_limit，连接保持可用。控制错误是 local 外层，不给 RpcError 加新 variant；建立失败不发 ack。建立后异步失败用 subscription 字段、无 request id，结束并释放资源。合法 subscription admission 走 E3d 同一审计 kernel，包括上述拒绝。
- [ ] details ack 后立即发当前 Some frame（T/unified_rpc.rs:549-565）；当前 None 时等待首个 Some，不伪造空帧。后续保持 watch-style latest-only coalescing，允许 sequence 跳跃，不虚称每帧必达，不为跳跃生成事件流 lag/reset。慢 writer 只保留当前/最新有界帧；unsubscribe/disconnect 释放 watch receiver/demand，ack 后不再 data，late receiver 走 E3d cleanup。E3b 只定义与测试帧/状态机，E3d 实施并执行（新）`local_details_requires_negotiated_feature`、`local_subscriptions_reject_unknown_stream_and_resource_limit`、`local_details_ack_precedes_current_frame`、`local_details_coalesce_and_release_demand`，必须经过 local dispatcher/真实连接，不只测 codec。

**验证**

- 新增（新）`local_frames_preserve_json_and_big_endian_lengths`、`local_codec_handles_fragmented_and_coalesced_frames`、`local_handshake_rejects_incompatible_versions_before_dispatch`、`local_required_features_are_negotiated`、`local_frame_limits_and_duplicate_ids_are_enforced`。`cargo test -p nyanpasu-rpc-protocol --no-default-features` 与 `--features frame-io` 各存 §0.8 清单，B/V。
- protocol 的 core/Tauri scoped 无匹配检查继续有效；`rg -n 'PROTOCOL_VERSION|MAX_FRAME|Hello|subscription' backend/nyanpasu-rpc-protocol/src` 正向审契约。反向绕过 handshake、删除长度上限、混用 ID，新增测试失败；不复制 local-channel 的同 build type-name fingerprint 作为协议协商。

## E3c — Unix 与 Windows 可发现端点

本卡是行为变更（新增持久监听基础设施），不自动启用桌面 local 服务。

- [ ] R/local/endpoint.rs 提供反复 accept 的 Unix socket/Windows named-pipe listener；借鉴 `backend/nyanpasu-local-channel/src/name.rs:77,237,255` 的当前用户 ACL、私有目录/owner/mode 校验。`tokio.rs:81` 的 OneShotServer 消费 self 不能直接当长期 listener。默认每用户、每 app data directory 只有一个端点；desktop/server 哪个先 bind 就拥有它，第二个 live binder 以 typed error 启动失败。服务端/client 共用 discovery，CLI 用默认或 `--endpoint`，不以 router `/var/run` 硬编码作为所有平台默认。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/local.rs` 的 accept `:68`、bind `:119`；丢弃任何 connect 错误都当 stale、固定 `.binding` 覆盖与无所有权校验 cleanup。Unix 仅在已校验的私有、当前用户目录内，现有项为当前用户拥有且 mode 合法的 socket、connect 明确 refused 时视为 crashed-owner residue 并在 bind 前删除；live、不可访问、foreign owner、wrong mode、symlink、目录外项全部拒绝。校验身份并串行化 stale 检查/claim，防 racing binder 删除新 live socket；正常 shutdown 只删除仍属本 listener 的端点，保留 replacement-safe cleanup。
- [ ] Windows named pipe 无 stale cleanup；首实例以 first-instance exclusivity 创建，保留当前用户 ACL，后续 accept instances 归同一 listener，竞争 shell 必须失败；不能用 non-Unix unsupported stub 达成 E3。共享端点错误为模块 snafu，本卡只复用 local-channel 的端点安全技术，在 RPC 端点 adapter 实现所需持久监听，不修改原 parent-child API/线协议；避免为一次性 channel 加服务端协议职责。
- [ ] G：rpc 新 `local-server` feature，依赖 protocol/frame-io 与实际 OS socket 库；新端点辅助不得引入 IMPLEMENTATION/core 到 protocol。无新包/prefix；musl 显式启用 rpc/local-server，同 target；local-channel 未改，不扩 musl 的 package/target 集合；其既有安全测试作为实现证据，RPC 新测试必须独立固定相同安全条件。更新 rpc/路线图平台支持表。

- [ ] 默认 endpoint identity 由当前 OS 用户 UID/SID 与规范化 app data directory 确定，不含 shell 类型/PID；Unix 在经 local-channel 同等安全校验的用户私有目录内，Windows 在当前用户命名管道命名空间。共同 discovery 函数固定该映射，两 shell/CLI 不能各自推导；`--endpoint` 只覆盖位置，不放宽安全校验。新增（新）`second_live_binder_is_rejected`、`racing_binders_have_one_winner`、`inaccessible_existing_endpoint_is_refused`、`crashed_owner_socket_is_reclaimed_only_in_private_directory`、`windows_pipe_name_competition_is_exclusive`、`desktop_and_server_share_default_endpoint_for_data_directory`；加到 §0.8/G/V 平台清单，Windows competition 必须 Windows 实跑。

**验证**

- 新增（新）`local_endpoint_accepts_consecutive_connections`、`local_endpoint_refuses_foreign_or_symlinked_paths`、`local_endpoint_cleanup_cannot_remove_a_replacement`、`named_pipe_accepts_reconnects_with_current_user_acl`。参考且不删除 local-channel 的 `a_loosened_fallback_dir_is_refused:316`、`a_symlinked_fallback_dir_is_refused:331`、`the_pipe_dacl_names_only_the_current_user:421`（name.rs）；§0.8/B/V，Windows named pipe 必须实际执行、Unix 权限由 Linux/macOS CI 验证。
- `rg -n 'remove_file|permissions|security_descriptor|accept' backend/nyanpasu-rpc/src/local --glob '*.rs'` 审所有删除/权限/循环点；无旧路径删除时不伪造零匹配证明。反向放宽 ACL/目录模式、把一次 accept 当长期服务或无条件 unlink，新测试失败。

## E3d — 连接 owner、订阅与统一审计

本卡是行为变更（新增 local dispatch/订阅），连接断开清理连接资源，但不取消 owner 已开始的工作。

- [ ] R/local dispatcher 使用服务端生成的 transport-tagged instance/connection 身份，P/RpcOwner 不接受 request JSON owner；原 desktop/window 与 HTTP/session 行为保留。参考 `git show 4a4a3a0d3:backend/nyanpasu-rpc-protocol/src/owner.rs` 的 `local_connection:21`，加实例命名空间，不复用重启后的 bare counter。
- [ ] 握手后每个调用进入 E2b 的 `dispatch_audited`，新增 Local transport，R 的共享生产 `TracingRpcAuditSink` 记 INFO；固定 command 名、脱敏 owner、耗时/结果，不记 params/result/token/error detail。目标/能力/参数拒绝同样审计；握手认证错误仍在 RPC kernel 外。special subscription establishment 只记一次，不逐帧审计。
- [ ] 有界 in-flight、输出 queue 及每连接 resource/subscription table（容量满时在 admission 前拒绝新增资源），slow peer 只影响该连接；建立资源表拥有 log sessions、bus subscriptions、connection-detail demand。迁/接通 `T/ipc.rs:1999,2012,2025` 对应 facade owner API；断开/取消订阅/shutdown 丢 receiver、关闭本 owner 资源，不能跨 owner 清理。所有成功返回资源 ID 的 resource-producing completion 即使连接已关闭仍登记结果并立即 close，包括 late subscription 与 App/Service `open_log_session`；completion task 活到登记/清理完成，不随 reply receiver 丢弃。`C/client/logs.rs:117` 需要 source/owner/returned session ID；已返回 ID 的成功完成不能靠 lease 弥补漏清理；未确认的远端 Service 创建例外见下述 V-61。
- [ ] 已接纳工作由 server task/业务 owner 持有，与 reader/writer 生命周期分开；root tasks 观测失败、正常 shutdown drain。panic 保留原 payload 并走 fatal supervision，不能 catch_unwind、吞 JoinError 或转 RpcError；不为 audit 另 spawn/timeout。丢弃 `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/local.rs` 的无界 queue `:200` 和 panic-to-internal `:248-267`，只参考 reader/writer 分离。
- [ ] G 无新包/prefix/target，local-server feature 的实际依赖闭包重验；新增审计 sink 的共享 enum 分支要求两个 shell 显式处理。更新 rpc/architecture/路线图连接生命周期与事件归属。

- [ ] 新增（新）`delayed_app_log_session_is_reclaimed_on_disconnect`、`delayed_service_log_session_is_reclaimed_on_disconnect`、`shutdown_reclaims_delayed_app_and_service_log_sessions`：barrier 卡住成功 open completion（Service 必须仍在 IpcServiceLogs 上游 5 秒窗口内返回 ID），断连/关闭后释放，断言立刻登记并调用真实 close(source, owner, returned ID)，session 不存在，不等待 lease；这只证明拿到 ID 的成功完成，不代表丢失 Service 回复也可立即清理。新增（新）`connection_resource_table_is_bounded`；Local INFO 检查必须使用 E2b 的生产 sink 与原 level/secret fixture，不能只计数 fake sink。

- [ ] 明确生产限制 V-61：`C/client/logs.rs:43-50,62-67` 的 IpcServiceLogs 内置 5 秒 timeout；服务可能已创建 session 却丢失/超时回复，local 只拿到 Unavailable、没有 ID，无法调用 `close:117`。此类未确认 Service open 依服务现有 45 秒 lease（`backend/nyanpasu-runtime/crates/nyanpasu-logging/src/session.rs:9,295-315`，从创建/最后续租计时）过期；不得宣称从 local 断连起立刻或固定 45 秒内完成所有物理 store 回收，store 另有 GRACE（:10）。不改 runtime submodule/service 协议、不新增 owner-close-all/correlation API。
- [ ] 新增（新）`service_open_lost_response_reports_unavailable_without_session_id`：使用实际 IpcServiceLogs 的受控 IPC peer，证明远端创建后回复丢失或超过 5 秒时只回 Unavailable、没有伪造 close ID；用受控时钟/时间推进，不 sleep。成功 late-completion 测试与此案例分开；现有 logging lease 实现为此限制的依据，不用最终返回 ID 的 fake port 证明未确认创建已回收。文档写清 lease bound 与前提（无续租），改变协议须另卡审议。

- [ ] 本卡实现 E3b 表中 reset builders、订阅 capability/error admission 与 details demand adapter，仅消费五个 full-value 流的现有 facade，不调用 public RPC query 来绕过 allowlist；执行 E3b 列出的四个 local dispatcher/details 测试及 full-value reset snapshot-race/非完整值流拒绝测试，全部按 §0.8 计入本卡新增清单。frame-only 的 E3b 不提前运行尚未存在的 dispatcher；client 端真实 overflow 重建验收在 E3e。

**验证**

- 新增（新）`local_dispatch_records_one_info_audit_event`、`local_owners_cannot_use_each_others_resources`、`disconnect_reclaims_late_subscriptions_without_cancelling_mutations`、`slow_local_peer_has_bounded_buffers`、`local_handler_panic_reaches_fatal_supervision`；barrier/Notify 驱动，不 sleeps。保留 C2f `audit_never_records_payloads_or_error_details:376`、`dropping_audited_call_preserves_cancellation:515`、`audited_command_panic_keeps_its_payload:567`；§0.8/B/V。
- `rg 'catch_unwind|unbounded_channel|RpcErrorKind::Internal' backend/nyanpasu-rpc/src/local --glob '*.rs'` 退出码 1；逐项审 JoinHandle result 不字符串化 panic。反向信任 JSON owner、断 reader 取消 owner、漏 late cleanup、降为 DEBUG/绕过 kernel，对应新测试失败。

## E3e — local client 与桌面接入

本卡是行为变更：显式启用 local 的桌面 shell 可被本机客户端调用；client 超时后连接失效，结果未知，不自动重试 mutation。

- [ ] P/client.rs 增加 `local-client` feature（依赖 frame-io，仅协议/OS client 库，无 rpc/core）；同一 transport 支持 Unix/Windows、握手、请求关联、订阅与断开。适配 `git show 4a4a3a0d3:backend/nyanpasu-rpc-protocol/src/client.rs` 的 call `:98`；替换 `call_with_timeout:110` 后继续读旧连接的策略，超时/半帧导致连接不可再用，旧回复不能归到新连接。
- [ ] 明确发送前连接失败与发送后结果未知；deadline 只在 client/transport，不下传取消 owner。所有可能已提交但未收到确定回复的 IO/decode/disconnect 失败保守报 unknown，不只把 TimedOut/Disconnected 当未知；不自动重放。
- [ ] 桌面 composition 显式配置 listener/enable 与 root shutdown，默认不开启，不增加前端控件。初版 local opt-in 限既有 `get_profiles`、`create_profile`、`delete_profile`、`read_profile_file`、`get_configuration_status`、`get_core_status`、`get_storage_item`、`set_storage_item`、`remove_storage_item`、`list_log_files`、`open_log_session`、`query_logs`、`close_log_session`（HEAD T/ipc.rs:392,494,617,702,2239,809,1714,1721,1732,1990,1999,2012,2025）；逐项复用 shared handler，不改变 HTTP 集合。local event/detail subscriptions 属传输控制帧，不把 native Channel 命令标 local。
- [ ] metadata 如实记录 mutation 的间接 runtime/service 效果；桌面 local 与桌面 client 共用其既有策略，不声称全局无提权；E4 的独立 server 负责拒绝。新增 durable-stop/host-bound-core/lifecycle RPC 不在本卡。local allowlist 扩展需独立行为卡。
- [ ] G：protocol/local-client 独立 tree 禁 core/rpc/server/GUI/IMPLEMENTATION；tauri 显式 local-server feature/配置，core/config defaults-off 保留。musl 同时测 protocol/local-client 与 rpc/local-server，仍同 lib targets；prefix 不新增。同步 rpc/路线图 allowlist、启用方式与 timeout 契约。

- [ ] 连接由独立 transport task 独占，call 通过有界 channel 提交请求并等待关联 reply；drop call future 不取消半帧 exchange，transport task 完成交换或丢弃无人接收的 reply，保持流同步。timeout/transport error 关闭连接，所有可能已提交的调用报告 outcome unknown，不自动 retry mutation；drop future 不触发重放。未提交的排队请求可取消，不能把不确定的 partial write 当未提交。新增（新）`cancel_during_partial_request_write_keeps_stream_synchronized`、`cancel_during_partial_response_read_keeps_stream_synchronized`，用可控 IO/barrier 分片，不 sleep；下一调用只能使用完整同步流或新连接，mutation 执行次数不增加。丢弃 POC `client.rs:110` 对借用 exchange 做 timeout 的连接所有权安排。

- [ ] 本卡 client 实现 E3b 的 reset 应用与独立 details data/error 解码；执行（新）`local_overflow_reset_reconstructs_each_recoverable_stream`，真实连接覆盖五个允许的 full-value event identities、无 query recovery、当前值整体替换，验证既有十三命令名单逐项不变。持续溢出/快照超过上限必须显式失败，不能收到 lag 就宣称恢复。按 §0.8 将本测试加入 client/server 两侧适用清单，B/V；Windows 与 Unix 实跑。

**验证**

- 新增（新）`timed_out_local_connection_cannot_be_reused`、`partial_reply_timeout_never_retries_a_mutation`、`local_allowlist_matches_the_reviewed_existing_commands`、`desktop_local_listener_is_opt_in_and_joins_shutdown`；适配 POC client 测试 `:172` 的 unknown 断言，丢弃 late-reply skip 后可复用断言。两平台实际 client/server roundtrip，§0.8/B/V。
- `cargo tree --offline -p nyanpasu-rpc-protocol --no-default-features --features local-client -e normal,build` 核对 client-only；`rg 'nyanpasu_rpc::|nyanpasu_core::|tauri::' backend/nyanpasu-rpc-protocol/src --glob '*.rs'` 退出码 1。反向超时后继续用 stream/自动重试/默认启用 listener，各新测试失败；未 opt-in 命令明确 unsupported。

---

## E1b-a — 完整 DTO 闭包与接收投影

> **修订（2026-11-11，V-67）**：本卡与 E1b-b 已按 `docs/audit/2026-11-07-rpc-contract-design.md` 精简实施，以 design §8.1 V-67 为准：闭包只覆盖 13 个 local 操作及其完整值流，普通值下沉 foundation 作唯一定义，protocol `types` 只含接收错误/mutation 形状与连接详情，绑定由 `RpcOperation` 契约编译期保证；下列清单中超出该范围的条目（全命令闭包、投影、AST oracle）不再实施。

本卡是保行为接缝，前置 D2e、A2b 与 E3e 全部完成；不再移动 D2e 的定义，不改业务结果或 wire。基础值保持 canonical；只有不适合基础层的应用输出/不可重建来源使用 protocol projection。

- [ ] 逐 registered command 建 request/result/error/event 闭包清单，含 native-only schema、cfg 分支与外部 crate 嵌套值。现有基础 DTO 直接引用；D2e 的 MutationOutcome/RuntimeError/ProfilesError 从 values 引用。Serialize-only 错误含 skipped operational sources 时，新建显式 receiving DTO，不伪造 IO/engine cause、不给 domain error 强塞 Deserialize、不把内部 MutationReceipt/RuntimeApplyReceipt 导出。
- [ ] profiles：T/ipc.rs:392-712；`C/state/profiles/actor.rs:66` NewProfileRequest、`client/profiles.rs:89` ProfileUrlProbe、`client/jobs.rs:184` ProfileSyncStatus 与 jobs DTO。config/runtime：T/ipc.rs:723-977、2239-2253；`C/client/runtime_inspection.rs:50,65,77,84`、`configuration_status.rs:15,27,34,42,55`、`state/config_error.rs:24`，effects/source status 的嵌套类型不能漏。core-control/status DTO 的 operational projection impl 留 core，协议投影在 R 转换。
- [ ] 其余组逐项闭合：Clash API/proxy（T/ipc.rs:1172-1309）、storage（:1714-1792，C/storage.rs:213,227）、streams/details（:1808,1927,1938,1951）、observers core logs（:1817-1846，`backend/nyanpasu-observers/src/logs/model.rs:14,35,120,129,140`）、traffic（:1853-1917）、日志 sessions/frontend batch（:1990-2038）、core updater（:1105,1162）、app updater（:2058-2134）、debug HTTP（:2561,2569）及 common IpcError/domain_error。须覆盖 HEAD 104 项；POC 是 85 个 extracted RPC HTTP 声明 + 20 个 retained shell 声明，共 105 项，不是小子集。
- [ ] native-only 值也登记：T/ipc.rs:178,186,207 的 dialog/filter、:1026 backup、:2048 ReleaseChannelInfo，及 CopyEnvOption/EditorWindowType/PageCapabilities。native Channel descriptor 是 shell binding，local 订阅帧不冒充该 DTO。每个 pure scalar/unit 命令也登记 error closure，不能因 result 简单漏项。
- [ ] 对仍属 CORE_FAMILY 的应用 DTO，P 定义 transport/receive projections，R 在应用边界转换；不把 observer/updater owner、config error 的操作 helper 移进 P，也不让 foundation 为 RPC 反向依赖 protocol。能直接引用的 canonical foundation 值不重复定义；wire projection 名与既有 Specta 名保持一致，接收专用类型不额外注册为新前端 export。POC `git show 4a4a3a0d3:backend/nyanpasu-rpc/src/commands.rs` 的 `create_profile:50` 的 core DTO 签名只作缺口证据，不复制为 protocol 依赖。
- [ ] G：protocol 按闭包新增批准的 foundation 值依赖（包括 runtime-values，config defaults-off）、必要中立外部 value 依赖；不依赖 CORE_FAMILY/IMPLEMENTATION/GUI。新增 gate 正例 CLI → protocol → values → config/contracts/transform；负例 protocol → core/engine/observers/updater，及 foundation → protocol/rpc 的倒置。不要复制 POC 的 CLI 禁全部 FOUNDATION 规则。prefix/musl package/target 保留，schema feature 显式转发，D2e 的值包注册不能被覆盖。
- [ ] 调用者为 R commands/error/events/http/local、T native DTO/schema 注册与 typed client；core 业务 API/操作 helpers 保持原 owner，删除被完全替代的 shell wire 定义/导入，不留转发。同步 backend-packages/路线图 E1b 状态，尚不宣布 typed-client 验收完成。

- [ ] 闭包显式包含 `get_core_agent_skills`（T/ipc.rs:1079）与 `C/client/agent_skills.rs:8` 的 Serialize-only `CoreAgentSkill`；P 的 projection/receiving shape 保留 `name`、`dir`、`installed`，R 转换，不能引用 core 类型或伪造 host 路径。新增（新）`core_agent_skill_projection_preserves_serialization`、`typed_client_decodes_core_agent_skills_without_core`，覆盖空/多项、路径与 bool；此类型不在 D2e move 清单，不能因已完成 D2e 漏掉。

- [ ] E2a/E2b 已把四个 HTTP/shared response（GetSysProxyResponse、IconData、ServiceStatusInfo、StorageEntry）移入 R，TrayIcon 已在 P；本卡不再声称它们是 shell/native-only 值或首次抽离。对四者仅完成仍有必要的向下投影及 receiving closure（尤其 ServiceStatusInfo 的 core 嵌套值），已中立且闭合的值可纯移动到 P，保持现有 wire/Specta 名；不能复刻同名 canonical 定义。

**验证**

- 新增（新）`every_registered_command_has_a_neutral_wire_closure`、`receiving_errors_decode_without_operational_sources`、`wire_projections_match_application_serialization`，按上述组参数化全部 variant/optional/default/serde tag。`cargo test -p nyanpasu-rpc-protocol`、`cargo test -p nyanpasu-rpc --features http,local-server`、`cargo test -p clash-nyanpasu`，§0.8 原清单加新测试，保留 T/specta_export.rs:728 的 shape assertions；B/V。
- `rg 'nyanpasu_core::|nyanpasu_state_engine|nyanpasu_observers|nyanpasu_updater|MutationReceipt|RuntimeApplyReceipt' backend/nyanpasu-rpc-protocol/src --glob '*.rs'` 退出码 1；旧 shell DTO 按实际迁移清单 scoped 检查，不全局禁止合法 domain 定义。反向嵌 core error/删除 receiving variant/改变空值或 source skip，tree/schema/接收测试失败。

## E1b-b — typed client 与闭包验收

> **修订（2026-11-11，V-67）**：本卡与 E1b-b 已按 `docs/audit/2026-11-07-rpc-contract-design.md` 精简实施，以 design §8.1 V-67 为准：闭包只覆盖 13 个 local 操作及其完整值流，普通值下沉 foundation 作唯一定义，protocol `types` 只含接收错误/mutation 形状与连接详情，绑定由 `RpcOperation` 契约编译期保证；下列清单中超出该范围的条目（全命令闭包、投影、AST oracle）不再实施。

本卡是保行为接缝；合并原独立的 client/schema integrity 验收，不新增业务命令。

- [ ] P 的 typed request/response decoding 接 E3e client；generic JSON call 保留，typed API 必须可在不链接 R/core 的消费 crate 单独使用。接收端 error 保留 RpcError kind/code/retryable/operation_id/domain_error，不能以成功 JSON 包裹失败。
- [ ] metadata/DTO 清单覆盖当前所有平台声明与实际 registry，生成绑定用同一命令声明来源；别名/同名 Serialize/Deserialize schema 不制造 frontend duplicate 类型。新接收类型只在 Rust client 使用时不加入现有 Specta export。
- [ ] G 无新包/prefix/musl target；protocol local-client 单包 features 与 rpc/server features 分别测，不靠 workspace feature unification。新增 policy 正例仅 protocol/client 的消费闭包，负例偷偷引入 rpc/server/core；同步 E1b 完成条件和路线图。

**验证**

- 新增（新）`typed_local_client_decodes_mutations_and_domain_errors`、`typed_client_builds_without_application_dependencies`；§0.8 protocol/R/T 清单不减，沿 E1b-a 完整闭包参数化回归，B/V。单独 `cargo test -p nyanpasu-rpc-protocol --no-default-features --features local-client`。
- client tree 及 E1b-a scoped 无匹配检查继续通过；反向给 client 加 core、把 error 当 Ok、遗漏接收嵌套 cause，策略/typed roundtrip 失败。三份 bindings 零 diff 是硬条件，不用「语义相同」接受文本 drift。

---

## E4a — 首次 headless 装配即拒绝提权

本卡是行为变更（新增 headless shell 装配）；不从 Tauri 复制出一个无拒绝策略的中间版本，不改变桌面策略。

- [ ] 新建 `nyanpasu-server` 的 H/compose.rs，按 `C/client/mod.rs:110` 的 ClientSetupArgs 装配一个 client。参照 HEAD `T/setup.rs:257,270,289,331,357,368,409,434,459,480,499,505,659,702` 的迁移、core/service、storage snapshot、geo/net/profile IO、runtime/script、logging/updater、traffic owner 与 shared effects；依赖从 shell 注入，不能把生产实现边加回 core/R。
- [ ] C1 的 app-update/window/accelerator 能力用 None，C3 shared effects 保留 logger/OS/PAC/proxy 收敛，presentation executor None；UiEventSink/FrontendLogSink/audit sink 用中立实现。A2b 已合入，server 显式构造其公开扩展工厂与 server-owned IO adapter，不导入 Tauri 的 script_extensions、不恢复 hidden 默认构造；无新共享交互 crate。
- [ ] 同一拒绝策略显式注入 `nyanpasu-os/src/service.rs:300`、`system_dns.rs:28`、`core_install.rs:31` 三构造器；使用 `elevation/mod.rs:9` RefuseElevation。真实 bundle metadata/path/executable source 从 server 参数/build metadata 来，不伪造 desktop 信息；保留 B2 owner 级 stores/factories 与 staging 生命周期。
- [ ] 参考 `git show 4a4a3a0d3:backend/nyanpasu-server/src/compose.rs` 的 assemble `:171`、shutdown `:148`；重写 old core adapters/concrete storage，丢弃 `adapters/effects.rs:17` UnsupportedEffects、假 windows/accelerators 和未注入拒绝的 DNS/install。POC 的 host-bound core/持久 stop/新 lifecycle 命令另立行为卡，不在本卡修改 runtime submodule。
- [ ] 直接与间接路径同卡固定：service-mode config 开/关、runtime retry、失败补偿、binary-copy fallback 经真实 client 到拒绝 adapter，计数 runner 证明提权命令未启动；允许普通非提权 IO，不声称 server 不做任何 OS 操作。嵌入式 desktop WebUI 仍走桌面策略（O-5）。
- [ ] G：workspace 加 server；shell 可依赖 core（defaults-off）、R/http/local-server、协议、基础/实现/owner crates；显式选 core/config/scripts features，不依赖 Tauri/GUI。policy 正例 server → core/OS/net，负例 server → Tauri/egui/Slint（含传递）；静态 prefix `backend/nyanpasu-server/src/`，musl 加 `-p nyanpasu-server`、`lib:nyanpasu_server`。本卡只 lib 组合根，二进制在 E4b；同步 development/路线图。

- [ ] 正向要求现行 concrete logging composition：`nyanpasu_core::logs::RedbCoreLogStore::open`（T/setup.rs:460，定义 C/logs/redb.rs:32、导出 C/logs/mod.rs:6），失败用当前 `nyanpasu_observers` 的 `UnavailableCoreLogStore`（T/setup.rs:466），并保留 FsLogFiles/MonotonicClock/IpcServiceLogs。禁止的历史 POC symbol 逐项为旧 owner 路径 `nyanpasu_core::logs::UnavailableCoreLogStore`、`UnsupportedEffects`，不能用 `nyanpasu_core::logs` 整个 namespace 当禁止表达式，也不要求额外存储提取。新增（新）`server_logging_uses_redb_with_current_unavailable_fallback`，反向删 fallback/换假 store 测试失败。

- [ ] 本卡补最小 OS 测试接缝：`backend/nyanpasu-os/Cargo.toml:9` 新增默认关闭 `test-support`，仅公开真实 service/DNS/core-install adapter 的 test constructors/factories 与必要 intercepting CommandRunner 接口。依据 `service.rs:300`、`system_dns.rs:28,48`、`core_install.rs:31,43` 与 `elevation/runner.rs:5,25-27`；不能把 cfg(test) 私有 CountingRunner 当跨 crate 可用接口。生产构造器和默认 OsCommandRunner 不变；测试 runner 能拦截并计数所有实际命令，RefuseElevation 仍注入真实 adapter。
- [ ] server 仅 dev-dependency 启用 `nyanpasu-os/test-support`，normal/build 边不启用、不由 server 生产 feature 转发；config/retry/compensation/copy-fallback 四条测试经真实 client + 三个真实 OS adapter/test constructors，断言 elevated runner 零调用，不用 fake refusal port 替代。无新增 crate/target，既有 OS/server 静态 prefix 与 musl 两清单保留；musl 的 server 测试装配显式包含此 dev feature，生产构建独立核验关闭。
- [ ] G 策略在 S/backend-boundaries.ts + backend-boundaries_test.ts 增 dev-only 正例、normal/build feature 直启/传递泄漏负例；S/policy.ts STATIC_GATE_PREFIXES 与 S/architecture-ledger_test.ts 覆盖已在 OS 前缀内的新模块，不新增盲区。`cargo tree --offline -p nyanpasu-server -e normal,build,features` 与独立 production metadata feature 闭包必须无 `nyanpasu-os/test-support`；test/all-targets feature-unified 图单独检查、不可当生产图。新增（新）`server_production_closure_excludes_os_test_support` 策略测试，反向 normal edge 启用该 feature 必须失败；按 §0.8 登记 seam 与四条真实路径测试，B/V，生产构造行为零变化。

**验证**

- 新增（新）`server_composes_without_presentation_capabilities`、`server_refuses_elevation_on_config_retry_compensation_and_copy_paths`（每条路径独立 case，不用只测 RefuseElevation 本身代替）。保留 OS `refused_service_command_never_spawns`（service.rs:361）、`refused_dns_flush_never_spawns`（system_dns.rs:144）、`refused_copy_fallback_never_spawns`、`install_request_keeps_staging_when_waiter_is_dropped`（core_install.rs:256,308）。§0.8 server 新清单单列，原 core/OS/R 清单无删；B/V。
- `rg 'tauri::|InteractiveElevationConsent|UnsupportedEffects|nyanpasu_core::updater' backend/nyanpasu-server/src --glob '*.rs'` 退出码 1；正向逐构造器查 RefuseElevation。反向任一构造器换 interactive、丢 compensation case、用 fake all-effects，新增对应 case/gate 失败；不宣称修复桌面嵌入式 HTTP。

## E4b — 长期 server 进程与关闭

本卡是行为变更：提供一个长期运行的 server，local 与可选 loopback HTTP 共享同一 client，会话不随请求重建。

- [ ] H/main.rs/run.rs 只解析显式目录/端点/HTTP 开关并调用 E4a 组合根；一次 try_new，复用 registry/client。启动 observations/Clash streams、E2c forwarders；listener ready 后才发布就绪，HTTP assets 可选，凭据/Host/Origin 原样，默认不打开 HTTP。无 LAN ticket。
- [ ] root token/task tracker 拥有所有 owner/forwarder/listener；启动失败反向收尾，shutdown 先停止接纳/关闭传输资源，再等 owner 工作与资源回收，不以网络 drain timeout 取消业务 owner。所有 JoinHandle panic 交 fatal supervision，普通 IO 失败是模块 snafu，不借 POC `.await` 错误日志吞 panic。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-server/src/run.rs` 的 run `:60` 与 serve `:70`；保留启动 streams 的意图，去除 WebTickets/RouterListener。endpoint discovery 与 client 共用 E3c 规则；不新增 core 生命周期 API。
- [ ] G：server 新 bin target `nyanpasu-server`，musl 两清单保留 package 并加 `bin:nyanpasu-server`，确认 `--lib --tests` 产物收集确实运行该 bin test；必要时显式 `--bins` 并登记全部新产物，不能忽略 unexpected target。prefix/边规则不变；同步 roadmap/server 使用契约。

- [ ] 在任何 runtime/owner/request task 启动前安装中立 process panic policy，fatal 通知通道/退出边界也先就绪；对齐 desktop `T/lib.rs:282` 安装的 panic hook 的非 GUI 语义：记录原 panic payload/位置，保留原 unwind/payload，不合成 RpcError；运行中 hook 通知 process supervisor 后返回，让原任务 unwind 并释放资源，随后 server 非零退出、不继续提供服务；退出边界不可用的启动期直接非零终止，对齐 desktop 无 AppHandle 分支（T/lib.rs:119-131）。不能只靠 root TaskTracker：逐项审 owned handles 与 Axum/library-spawned request tasks，覆盖原 `T/server/debug_http.rs:163,211-214` 的 spawn/丢 handle 及 `:82-87` shutdown join；panic hook 覆盖库内部 task，所有可持有 handle 的 panic 继续向 fatal supervisor 传播，不能吞 JoinError。
- [ ] 新增（新）`http_handler_panic_terminates_server_process`、`local_handler_panic_terminates_server_process`、`background_owner_panic_terminates_server_process`：在真实 server 子进程注入唯一 panic payload，分别触发三条路径，断言非零退出、日志保留 payload/位置、没有 RpcError response。测试父进程有受控 watchdog 防挂死，不能把 watchdog kill 算通过；证明 unwind/payload 传播仍由原 `audited_command_panic_keeps_its_payload` 覆盖。lib/bin 测试清单登记 subprocess 三例，进程证据不能用 spawned unit task 代替。

**验证**

- 新增（新）`server_builds_one_application_for_all_transports`、`server_readiness_waits_for_listeners_and_forwarders`、`server_shutdown_drains_admitted_owner_work`、`server_startup_failure_releases_bound_endpoints`；显式 barrier，不 sleeps。§0.8 lib/bin 清单登记，B/V；两 OS 的 listener/startup/shutdown 行为 CI 运行。
- `rg 'WebTickets|RouterListener|ticket=|catch_unwind' backend/nyanpasu-server/src --glob '*.rs'` 退出码 1，正向审 try_new 只在组合根；反向每连接建 client、漏启动 stream、先 abort owners，新增测试失败。

## E4c — 只连接既有 shell 的 CLI

本卡是行为变更（新增 CLI），不在 CLI 内启动或恢复应用图。

- [ ] 新建 nyanpasu-cli，L/lib.rs/main.rs 提供 `call <method>`、JSON params/params-file、endpoint/timeout 参数；复用 E3e client 和 E1b 接收类型。无参数时的 usage/错误明确；不增加 lifecycle/reconcile/redacted-YAML 等业务命令，不为 CLI 加通用 service locator。
- [ ] 适配 `git show 4a4a3a0d3:backend/nyanpasu-cli/src/lib.rs` 的 parse `:92`、execute `:185`、transport_failure `:220`；保留 JSON stdout、result exit 0、RpcError exit 1、usage/transport exit 2，诊断写 stderr。去掉 router-only 默认路径，使用 E3c discovery。超时连接失效、outcome_unknown，不自动重试。
- [ ] G：生产依赖 protocol/local-client 与参数/JSON/错误/async 工具，不链接 R/server/core/IMPLEMENTATION/GUI；foundation 经 protocol 的值闭包允许。gate 正例 CLI → protocol → values，负例 CLI → rpc/server/core/engine/OS/Tauri 直接或传递；不能照搬 POC 对 FOUNDATION 的禁令。core/config 的新边不存在，若 transitively 由 protocol 使用仍 defaults-off。
- [ ] 静态 prefix `backend/nyanpasu-cli/src/`，musl 两表加 `-p nyanpasu-cli`、`lib:nyanpasu_cli`、`bin:nyanpasu-cli`；无 integration target 到 E4d。独立客户端 crate 暂不建，只有 feature 隔离不足再审。同步 backend-packages/architecture/路线图 CLI 层。

**验证**

- 新增（新）`cli_success_and_rpc_error_keep_distinct_exit_codes`、`cli_timeout_reports_unknown_without_retry`、`cli_never_constructs_an_application`、`cli_endpoint_override_works_on_unix_and_windows`；POC `lib.rs:364,384,400` 仅为参考测试，不冒充 HEAD 清单。`cargo test -p nyanpasu-cli`，§0.8 新 lib/bin inventory，B/V。
- `cargo tree --offline -p nyanpasu-cli -e normal,build` 满足 gate；`rg 'NyanpasuClient|nyanpasu_core::|nyanpasu_rpc::|nyanpasu_server::|tauri::' backend/nyanpasu-cli/src --glob '*.rs'` 退出码 1。反向把 transport error 当业务错误/输出成功或偷偷构造 core，测试/gate 失败。

## E4d — 连续 CLI 调用的同会话验收

本卡是保行为接缝（集成验证），不新增公开测试命令或生产 session API。

- [ ] 新建 `backend/nyanpasu-cli/tests/session.rs`，通过 server 的受控 ready/shutdown handle 启动一个长期服务，再运行真实 CLI 子进程；一次 create profile、另一次查询同一结果，并用 test-only composition counter 或 volatile owner/session 证据证明不是各自从磁盘重建。生产依赖不因 fixture 加 server/core。
- [ ] 同测两连接不能访问彼此 log session/subscription；连接 A 的 admitted mutation 被 gate 暂停，断开 A 后释放 gate，从 B 观察完成；late subscribe/disconnect 回收 demand。每个等待使用 readiness/barrier/Notify，测试 deadline 只作失败边界，不在生产 owner 加 timeout。
- [ ] `git show 4a4a3a0d3:backend/nyanpasu-cli/tests/session.rs` 的 `consecutive_calls_observe_the_one_server_session:44` 的连续进程场景可借鉴，`:56` 的无参 server.rpc 已与 POC compose.rs:133 不符，`:60` 的 sleep readiness 不保留；不声称该 POC fixture 已通过本栈测试。
- [ ] G：CLI dev-dependency server 的 test-support 按 §0.5 最小开放，仅 fake adapter/readiness/计数组合接口，生产 graph 不变；musl 保留 CLI package 并加实际 integration identity `test:session`，新增 integration artifact 不忽略。prefix 已覆盖生产源码；现有测试扫描策略照旧，positive production/negative leaked normal-edge tests 保留。同步 E4 完成证据，E5 仍暂停。

**验证**

- 新增（新）`consecutive_cli_processes_observe_one_server_session`、`cli_disconnect_does_not_cancel_an_admitted_mutation`、`separate_cli_connections_cannot_reuse_owned_resources`；`cargo test -p nyanpasu-cli --test session`。§0.8 单列 integration 清单，lib/bin 清单不减；Windows CI 跑命名管道，Linux/musl 跑 Unix socket，macOS CI 跑 Unix路径/关闭；B/V。
- production tree 与 E4c scoped rg 仍无禁边；反向每次 call 装配新 client、断连取消 owner 或共享 owner label，分别使三例失败。只观察同一持久文件不能替代同会话断言。无公开 wire/生成绑定/前端 diff。

---

### 延期项：local invalidation/storage 恢复（非 E3 卡，须另经审议）

此项不加入当前 PR 栈、测试清单或 E3 完成判据；未来暴露 `state-changed-event`、storage 或其他非完整值流，须先建立独立行为卡。三轮审计 findings 1–3 是该卡的最低验收条件：

- 普通 state invalidation 在 initial reset 之后、没有 local overflow 时仍能更新订阅状态（可有界合并为 fresh reset）；storage source receiver 的 Resync 必须重置每个受影响 storage 订阅，不依赖订阅者选择另一 identity。真实连接分别测普通 invalidation 与 local 快速消费时的上游 source overflow，不能用 writer overflow 替代。
- 本地 live storage 映射须过滤非 `web:` key，保留既有 event payload 表示，接收端将带 `web:` 的 key 归一到 reset map 的 namespace，并一致解释 raw JSON-string value；真实连接验证 reset → update → removal 同一 key、内部 key 从不交付。不能改变 Tauri/SSE payload。
- storage snapshot 必须在现有 revision 同步下同时返回数据与 revision（水位仅属于恢复边界，不改既有 HTTP/Tauri result）；丢弃 snapshot 已覆盖的旧通知。确定性延迟旧 update/deletion 到较新 reset 之后，状态不得倒退。证据：C/storage.rs:254,374,412,440,459,466；T/setup.rs:341,349；frontend/query/src/provider/kv-storage-state.ts:52、mutation-provider.tsx:14,92。

这些未来工作不扩当前十三命令 allowlist，不改变 admission-only audit。其他被排除流也需独立定义恢复语义后再审议，不能因已有 bus identity 自动开放。

## Phase B–E 里程碑卡

每张卡开工前先细化为与 Phase A 同粒度的任务并审阅；B1a–B4 已细化为 §2 的卡，C1–C3 已细化为 §3 的卡，A2b 与 D1/D2 已细化为 §4 的卡，E1a–E4 已细化为 §5 的卡；E5 仍仅保留暂停里程碑。依赖图、决策、前置、不变量、完成判据见 design §5。

| 卡  | 内容                                                                                                                                                            | 前置                      | 完成判据（摘要）                                                                                                      |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| A2b | 扩展注入；默认双引擎 feature；typed 不支持经 pipeline 到 wire（已交付 PR-A2b；musl/浏览器执行见 §4）                                                            | A2；合入在 D4 后          | 注入能力/条件确定性；四组 feature、禁用 Lua 不作 false/no-op；bindings/六语言/前端回归已落地；浏览器执行待 Playwright |
| B1a | persistence 收 format 与通用原子写；`session_state` 不依赖 migration                                                                                            | A5b                       | 无 `session_state → migration`；两个恢复测试通过                                                                      |
| B1b | backup/migration 的存储端口；migration crate                                                                                                                    | B1a                       | migration crate 不依赖 redb                                                                                           |
| B2  | facade 签名去具体存储类型；traffic legacy 导入器 feature 化                                                                                                     | B1b                       | `ClientSetupArgs` 类型闭包无 `Storage`/redb；traffic 无默认 redb                                                      |
| B3  | logs、traffic、geo 成 owner crate；`owner_shutdown` 按需建 crate                                                                                                | B2                        | owner 生产代码不含 clash/core-control/redb/notify；traffic 回归测试通过                                               |
| B4  | updater crate；后端与其 HTTP 客户端由 shell 注入                                                                                                                | B3a                       | facade 不构造 `HttpUpdaterBackend`；facade 无归档依赖                                                                 |
| C1  | 展示能力可选、结构化不支持（已细化为 §3 C1a、C1b）                                                                                                              | A                         | 无展示能力的装配测试；显式热键编辑在提交前被拒                                                                        |
| C2  | `nyanpasu-os` 与提权同意端口；能力端口进 contracts；`update_core` 去掉 HTTP；RPC 审计切面（已细化为 §3 C2a–C2f）                                                | A、B4                     | core 无提权执行点；拒绝策略不启动提权命令；经 HTTP 的 `update_core` 得到不支持                                        |
| C3  | 桌面交互机制移入 tauri 外壳；共享收敛与展示效果分离（已细化为 §3 C3a）                                                                                          | C1                        | core 族不含桌面机制、不直接依赖 widget-protocol；效果与热键回归测试通过                                               |
| D1  | 完整泛型 `nyanpasu-state-engine`，CORE_FAMILY 叶（已交付 #6224 PR-D1；musl 执行待 Docker 环境）                                                                 | B1a                       | 102 例名称清单不变且通过；父 workspace 仅 persistence；gate/static/musl 同注册                                        |
| D2  | 源工厂；独立严格结算行为修正；上层 foundation runtime-values；control/application 模块边界（已交付 #6229 PR-D2、#6231 PR-D3、#6232 PR-D4；平台/musl 执行见 §4） | D1、B3、B4、C1b、C2f、C3a | 无源 → 具体 workflow client；终态在源决定后；值包依赖与源导入策略生效；D2d 仅一行 JSDoc diff，wire 不变               |
| E1a | 宏接缝/独立目标与能力 admission；protocol 信封（已细化为 §5 的卡 E1a-a–d）                                                                                      | A；实施在 PR-A2b 后       | 中立展开无 Tauri；缺能力 unsupported；bindings 零 diff                                                                |
| E1b | 完整 DTO 低于 core 族，canonical 基础值/receiving 投影（已细化为 §5 的卡 E1b-a/b）                                                                              | E3e、D2e、A2b             | 独立 typed client；全部命令含 cfg/native 闭包；bindings 零 diff                                                       |
| E2  | 中立 registry/commands/HTTP/audit/bus（已细化为 §5 的卡 E2a–c）                                                                                                 | E1a、C2f                  | 无 Tauri 类型；audit 恰一次；bus-first 无回声、lag/resync                                                             |
| E3  | API-only、协商/两平台 local/连接资源/client（已细化为 §5 的卡 E3a–e）                                                                                           | E2                        | 两 allowlist；INFO audit；timeout invalidate；owner 隔离/断连不取消工作                                               |
| E4  | 拒绝提权的长期 server、小 CLI 与同会话集成（已细化为 §5 的卡 E4a–d）                                                                                            | C2d、C3a、E1b、E3；A2b 后 | server/CLI 无 GUI；CLI 无 core 族/实现/rpc/server；仅独立 server 关闭 #6157 剩余路径                                  |
| E5  | OpenWrt/LAN ticket login 暂停；§5 不设实施卡                                                                                                                    | E4 完成且用户明确恢复     | 恢复后另细化授权、procd、打包与硬件，musl 不替代                                                                      |

B 结束时按 design §6 复测编译时间。

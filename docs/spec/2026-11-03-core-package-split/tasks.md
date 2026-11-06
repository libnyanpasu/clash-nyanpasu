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

- [ ] `S/backend-boundaries.ts`：用具名集合（`GUI`、`FOUNDATION`、`CORE_FAMILY`、`IMPLEMENTATION`）加 `{ package, forbids }` 规则表替换 `NEUTRAL_PACKAGES` 与 `dependencyViolations` 里的 `if` 链；检查某包时跳过 `cargo tree` 输出中的根包自身；`cargo tree` 参数不变。
- [ ] 本卡只表达现有规则：GUI 族对所有中立包、core → platform、diagnose → core/platform。集合中尚不存在的包不出现在表里。
- [ ] `S/backend-boundaries_test.ts`：每条规则一正一反；新增"根包自身不算违规"与"禁止 `nyanpasu-local-channel` 拦住 widget `ipc` feature"的用例；保留"platform → core 允许"的现有用例直到 A5d。

**验证**

- `pnpm run test:scripts` 通过；`pnpm run lint:backend-boundaries` 在本分支通过。
- 反向检查：临时删掉规则表里 core → platform 那行，对应策略测试变红。

## T0b — feature 接线

- [ ] 以下每条边加 `default-features = false` 并显式列出所需 feature：tauri → core（`backend/tauri/Cargo.toml:39`，dev `:271` 保留 `test-support`）、platform → core（`backend/nyanpasu-platform/Cargo.toml:16`，dev `:65`）、core → config（`backend/nyanpasu-core/Cargo.toml:77`）、platform → config（`:19`）、tauri → config（`backend/tauri/Cargo.toml:41`）、diagnose → config（`backend/nyanpasu-diagnose/Cargo.toml:10`）。
- [ ] tauri 的 `default-meta`、`nesting` 转发（`backend/tauri/Cargo.toml:285-299`）保持语义：tauri 默认 feature 仍打开它们。
- [ ] 不改 `test-support` 经 dev-dependency 打开的做法（O-3）。

**验证**

- 修改前后用同一组参数各执行一次 `cargo tree -p clash-nyanpasu -e features -i nyanpasu-core` 与 `-i nyanpasu-config`（默认 feature），两次输出的 feature 集合一致；差异写进 PR 描述。
- `cargo tree -p clash-nyanpasu --no-default-features -e features -i nyanpasu-config` 与 `-i nyanpasu-core` 中都不出现 `default-meta`（同一组参数下检查，修改前该命令会出现）。

## T0c — 构建基线

- [ ] 按 design §6 测：干净 `cargo build --timings -p clash-nyanpasu`，四个增量场景各 3 次取中位数；记录环境字段。
- [ ] 额外记录 `cargo build -p clash-nyanpasu` 与 `cargo test -p clash-nyanpasu --no-run` 各编译几份 `nyanpasu-core`（`--timings` 报告中的单元），作为 O-3 的数据。
- [ ] 结果写进 PR-T0 描述，附总时长、core 单元时长、关键路径。

**验证**

- PR 描述含四个场景的三次原始数据与中位数，环境字段齐全。

---

## A1a — `owner_shutdown` 移到 core 根模块

- [ ] `C/client/owner_shutdown.rs` → `C/owner_shutdown.rs`，在 `C/lib.rs` 声明，删除 `C/client/mod.rs:6` 的声明；内容不变。
- [ ] 更新全部使用者：`C/client/{app_lifecycle.rs,app_update/mod.rs,application.rs,application_workflow/mod.rs,clash_config.rs,effects/actor.rs,hotkey/mod.rs,profiles.rs,session_state.rs,system_proxy/mod.rs}`、`C/actor_v2/service_actor.rs`、`C/clash/ws.rs`、`C/geo/client.rs`、`C/logs/actor.rs`、`C/proxies.rs`、`C/traffic/client.rs`、`C/updater/mod.rs`、`T/setup.rs:4`。

**验证**

- `rg 'client::owner_shutdown' backend/nyanpasu-core backend/tauri` 退出码 1。
- `cargo test -p nyanpasu-core` 通过，`C/client/app_lifecycle.rs` 的关闭测试在测试名清单中。

## A1b — 删除 `ScriptType`，统一用 `ScriptRuntime`

- [ ] 删除 `C/enhance/mod.rs:8-53` 的 `ScriptType`、`ScriptWrapper` 及其测试；`ScriptWrapper` 在 `P/enhance/script/` 内重新定义为私有类型（`ScriptRuntime` 与 `String`），`RunnerManager::process_script` 收窄为模块内可见。
- [ ] `P/enhance/script/{adapter.rs,runner.rs}` 改用 `nyanpasu_config::profile::ScriptRuntime`；删除 `P/enhance/script/mod.rs:8` 的 re-export 与 `:12,16` 的注释代码。
- [ ] 若 config 没有 `ScriptRuntime::JavaScript` 序列化为 `"javascript"` 的测试，把被删的测试改写为该断言放进 `Q/profile/definition.rs` 的测试；已有则不补。

**验证**

- `rg 'ScriptType' backend/nyanpasu-core backend/nyanpasu-platform` 退出码 1。
- platform 的 script 测试名清单与改动前一致且通过；`cargo test -p nyanpasu-config` 通过；三个生成文件无 diff。

## A1c — 新建 `nyanpasu-contracts`，移入 `BundleMetadata`

- [ ] 新建 `backend/nyanpasu-contracts`，依赖只有 `nyanpasu-config`（关默认 feature）；`lib.rs` 只含 `bundle_metadata`。
- [ ] `C/client/bundle_metadata.rs` 移入；`TEST_BUNDLE_METADATA` 改为 `#[cfg(feature = "test-support")]`；core 的 `test-support` 转发 `nyanpasu-contracts/test-support`，core 加 dev-dependency `nyanpasu-contracts = { …, features = ["test-support"] }`；platform、tauri 的 dev 依赖同样开启。
- [ ] 更新全部使用者：core 的 `client/{mod.rs,app_update/mod.rs,app_update/tests.rs,session_state.rs,startup_recovery_tests.rs,test_support.rs,test_support/facade.rs,test_support/startup_recovery.rs}`、`backup.rs`、`migration/{mod.rs,runner.rs,modules/profiles.rs}`、`service/icon.rs`、`updater/instance.rs`、`utils/{candy.rs,net.rs}`；platform 的 `changelog_http.rs`、`profiles/{fs_service.rs,test_support.rs,fs_service/integration_tests/profiles.rs}`；tauri 的 `adapters/app_update.rs`、`bundle.rs`、`bundle/tests.rs`、`setup.rs`。
- [ ] 登记：workspace members；`FOUNDATION` 集合加 contracts，contracts 的附加禁止项（`ractor`、`reqwest`、`redb`、`nyanpasu-core-manager`、`nyanpasu-local-channel`）与策略测试；`STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-contracts/src/` 与覆盖测试。

**验证**

- `rg 'mod bundle_metadata' backend/nyanpasu-core/src/client` 退出码 1（旧位置的模块声明已删）；`rg 'client::bundle_metadata|client::BundleMetadata' backend` 退出码 1（旧限定路径无残留；不查字段名，不禁止 contracts 中的新声明）。
- 单独 `cargo test -p nyanpasu-core` 与 `cargo test -p nyanpasu-platform` 通过（证明 dev 侧 `test-support` 接好）。
- `pnpm run lint:backend-boundaries`、`pnpm run test:scripts`、`pnpm run test:architecture-ledger` 通过；反向检查：给 contracts 临时加 `ractor` 依赖，门禁失败。

---

## A2 — `nyanpasu-scripts` 与 runner 构造闭包

- [ ] core `C/client/application_workflow/ports.rs` 增加 `ScriptRunnerFactory` 类型（design §4.2），文档注释写明在阻塞线程上调用、runner 用完即销毁。
- [ ] 新建 `backend/nyanpasu-scripts`，`P/enhance/script/` 七个文件整体移入；依赖带上 `backend/nyanpasu-platform/Cargo.toml:20-21,39-55` 中的脚本依赖与这些文件实际用到的共享依赖；依赖只有 config 与这些库。
- [ ] `P/enhance/runtime_build.rs`：`FsRuntimeBuildAdapter` 的 `ScriptDirs` 字段改为 `Arc<ScriptRunnerFactory>`，`:82` 改为在阻塞闭包内调用；`StartScriptRunner` 的 source 从 `io::Error` 改为 `PortError`。
- [ ] `P/enhance/runtime_build/tests.rs:60-65,89-93` 改为经闭包构造 runner（platform dev-dependency scripts）；golden 与 `backend/nyanpasu-platform/tests/runtime_builder.rs` 同样处理。
- [ ] `T/setup.rs:357-371` 构造闭包并注入；tauri 加 scripts 依赖；platform 删除脚本引擎依赖。
- [ ] 登记 scripts：workspace、`IMPLEMENTATION` 集合、`STATIC_GATE_PREFIXES`。

**验证**

- scripts 的测试名清单等于移动前 platform 中 `enhance::script` 下的测试名清单（模块前缀不同，名字一致）且全部通过。
- platform 的 golden 7 例、`runtime_build/tests.rs`、`tests/runtime_builder.rs` 通过。
- 门禁通过；反向检查：给 scripts 临时加 core 依赖，门禁失败。

## A3 — `nyanpasu-profile-transform`

- [ ] 新建 `backend/nyanpasu-profile-transform`，移入 `C/enhance/{runtime_builder.rs,chain.rs,utils.rs,builtin/}`，`LogSpan` 公开导出；`test_support.rs` 门控为 `#[cfg(any(test, feature = "test-support"))]`，`metadata` 等被 core 测试使用的夹具改为 `pub`。
- [ ] `C/enhance/artifact_snapshot.rs` 移到 `C/client/` 下 `runtime.rs` 旁（模块名按内容取），改用 transform 的 `LogSpan`；删除 `C/enhance/` 目录与 `lib.rs` 声明。
- [ ] 更新调用方：`C/client/{runtime.rs,runtime_error.rs,application_workflow/ports.rs,runtime_inspection.rs,test_support/runtime_build.rs,application_workflow/impact.rs}`、`C/client/application_workflow/tests/{mod.rs,connection_policy.rs,mutations.rs}`、`C/state/profiles/{sources.rs,actor.rs}` 的测试、`P/enhance/runtime_build.rs`、`P/enhance/runtime_build/{tests.rs,golden.rs}`、`backend/nyanpasu-platform/tests/runtime_builder.rs`、`T/ipc.rs:19`。
- [ ] core 加 transform 依赖；core 的 `test-support` 转发 transform 的 `test-support`，并加 dev-dependency 开启它；platform 的 dev 依赖同样开启。
- [ ] 登记 transform：workspace、`FOUNDATION` 集合（附加禁止 `nyanpasu-contracts`、`nyanpasu-persistence`、`nyanpasu-scripts`，及 config 不得依赖 transform）与正反策略测试、`STATIC_GATE_PREFIXES`。

**验证**

- `rg 'crate::enhance' backend/nyanpasu-core/src` 退出码 1；`rg 'nyanpasu_core::enhance' backend/nyanpasu-platform backend/tauri` 退出码 1。
- 门禁通过（transform 不依赖 core 族、实现 crate）；三个生成文件无 diff。
- 单独 `cargo test -p nyanpasu-core`、`-p nyanpasu-platform`、`-p nyanpasu-profile-transform` 通过，transform 的测试名清单等于移动前 core 中对应模块的清单。

---

## A4a — contracts：profile 与 materialization 端口

- [ ] 改动前，在当前提交上为每个将被替换的错误变体生成期望值（序列化 JSON、`Display`、逐级 `source()` 文本，用真实 reqwest/atomicwrites/serde_yaml/corrupt 错误构造），写成测试，确认通过。
- [ ] 移入 `C/state/profiles/ports.rs` 的端口与值类型，以及 `error.rs` 中只属于这些端口的错误（`ProfileFileError`、`SubscriptionFetchError`、`ErrorPath`、`ExpectedNode`）；`ProfilePathError` 留在 config；`ProfilesError` 与事务错误留在 core。
- [ ] contracts 定义 `BoxedCause`；`ProfileFileError` 的 `corrupt::Error`、`atomicwrites::Error`、YAML 错误字段与 `SubscriptionFetchError` 的三个 `reqwest::Error` 字段改为 `BoxedCause`，`#[serde(skip)]` 不变；实现方在产生处装箱。
- [ ] `ProfileDegradationCode::retryable` 改为 `pub`；`SubscriptionFetchError::mock` 改为 contracts `test-support` 下的公开构造器，`C/client/mod.rs:2974,3134` 等测试改用它；`C/client/test_support.rs:48,784` 用到的 mock 随 trait 由 `automock` 生成并在 `test-support` 下导出。
- [ ] 更新全部调用方（core 的 profiles、jobs、client 及测试；platform；tauri `ipc.rs` 测试）。

**验证**

- 第一步写的期望值测试在改动后原样通过（替换后的变体 JSON、`Display`、source 链不变）。
- `T/ipc.rs:2359-2390`、`frontend/nyanpasu/tests/ipc-error.browser.test.ts` 通过；三个生成文件无 diff。
- `backend/nyanpasu-contracts/Cargo.toml` 不声明 `reqwest`、`atomicwrites`、`serde_yaml*`；门禁通过。
- 反向检查：把某处装箱改成 `BoxedCause::from(e.to_string())`（能编译，但丢失下层 source 链），期望值测试变红。

## A4b — contracts：网络端口

- [ ] 移入 `ChangelogHttp`（`C/client/app_update/changelog.rs:46-50`）、`SelfProxyPortSource`（`C/service/profile_file/ports.rs:3-8`）、`DirectEgressProbe`（`C/client/direct_egress.rs:25-31`）；`SessionPortResolver`（`C/client/ports.rs:232`）留在 core 实现它。
- [ ] 新增三个消费端口：`IconFetcher`（`C/service/icon.rs:35-83`，返回 `bytes::Bytes`）、网络诊断（`C/utils/net.rs` 的 `url_delay_test`、`get_ipsb_asn`，调用方 `C/client/mod.rs:793,803`）、`SystemProxySource`（替代 `C/utils/config.rs:11-19` 的直接调用，实时读取）。现有函数的 `PathResolver` 与元数据改为实现的构造依赖；self proxy 端口仍按每次调用传入（facade 照旧用 `self.clash_info().port`，`C/client/mod.rs:796,803,813`），不在构造时固定。
- [ ] 图标抓取、网络诊断的现有实现移到 core 内新模块 `C/net_adapters.rs`（A5c 整体移入 net）；图标服务与 facade 方法改为持有端口；tauri `setup.rs` 构造并注入。
- [ ] 更新调用方与测试实现（`T/unified_rpc.rs:863` 等）。

**验证**

- `rg -l 'get_reqwest_client' backend/nyanpasu-core/src` 只列出 `utils/candy.rs` 与 `net_adapters.rs`（图标服务与 facade 不再自己建客户端）。
- 新增替身测试（图标服务目前没有测试，本卡新建）：图标服务与两个诊断方法各一条，断言替身被以期望参数（含当次的端口）调用；图标抓取与 ASN 查询的错误原样传到调用方，延迟测试的 `Some`/`None` 原样传到调用方（公开返回形状不变）。另加一条：两次调用之间改变 `clash_info` 的端口，替身两次收到的端口不同。
- egress 与 changelog（`C/client/app_update/changelog.rs:318-335`）的现有测试通过。

---

## A5a — 运行时构建编排上移 core

- [ ] core 新增 `RuntimeBuildService`（`C/client/` 下），字段 `Arc<ScriptRunnerFactory>`、`Arc<dyn ProfileContentSource + Send + Sync>`、`RuntimePaths`、core-spec 解析器；实现 `RuntimeBuildPort`。逻辑从 `P/enhance/runtime_build.rs:37-107` 原样迁入（含 async 方法内直接读内容、`ProfilesActor::current_closure`、`blocking::join`、`write_product`）。
- [ ] `T/setup.rs` 改为构造 `RuntimeBuildService`（闭包来自 scripts 构造函数，内容源仍来自 platform 的 `content_source.rs`）。
- [ ] 删除 `P/enhance/runtime_build.rs`；`P/enhance/runtime_build/tests.rs` 迁入 core，内容源换成测试内的内存实现、runner 换成测试替身；若某用例必须真实引擎，core 加 scripts 的 dev-dependency 并在 PR 说明。
- [ ] golden（`P/enhance/runtime_build/golden.rs` 与 fixtures）与 `backend/nyanpasu-platform/tests/runtime_builder.rs` 迁入 transform 的 `cfg(test)` 单测模块，transform dev-dependency scripts；夹具经 `cfg(any(test, feature = "test-support"))` 直接可用；内容源用测试内实现读 fixture 目录。

**验证**

- 迁移前后 fixture 文件的 `git hash-object` 一致；单独 `cargo test -p nyanpasu-profile-transform`（不经 workspace 其他包的 feature 请求）下 golden 测试名清单为 7 例且通过。
- core 下严格/宽松用例（原 `P/enhance/runtime_build/tests.rs:72-124`）通过。
- `rg 'nyanpasu_core' backend/nyanpasu-platform/src/enhance` 退出码 1。

## A5b — persistence 与 profile-io（文件部分）

- [ ] 新建 `backend/nyanpasu-persistence`：从 `C/corrupt.rs` 移入 `retry_io`、`retry`、`retry_with_delay`（改为 `pub`）、`retry_async`、`is_link`、`preserve`、`Error`、时间戳格式化与独占重命名辅助及其测试；依赖 `backon`、`nyanpasu-utils`（不开 `redb`）、snafu、`time`、tracing、`atomicwrites`、macOS `nix`。`error_io` 与 redb、core-manager 分类留在 core。更新 core 内全部使用者，包括 `retry_with_delay` 的生产调用点 `C/session_state.rs:48`、`C/storage.rs:277`（直接导入，不留转发函数）以及 `migration/{fs,store,child_result}.rs`、`state/profiles/error.rs`。
- [ ] 新建 `backend/nyanpasu-profile-io`：移入 `ProfileFsPort`、`ProfileMaterializationPort` 的实现（`P/profiles/{fs_service,materialization}.rs` 对应部分）与 `P/enhance/content_source.rs`；依赖 contracts、config、persistence。订阅抓取留在 platform，tauri 的 `ProfileFilePorts.fetcher` 暂由 platform 提供。
- [ ] 同一提交迁完仍在 platform 的测试装配：`P/profiles/test_support.rs:22-30` 改为 profile-io 的实现填 `fs`、`materialization`、platform 的抓取类型填 `fetcher`；`P/profiles/fs_service/integration_tests/{profiles,facade,workflow}.rs` 中把旧类型当作文件或 materialization 端口的地方（如 `profiles.rs:143`）改用 profile-io 的类型；platform 加 profile-io 的 dev-dependency。
- [ ] adapter 本地测试随文件进入 profile-io，夹具改用 contracts `test-support`。
- [ ] 登记：`FOUNDATION` 加 persistence（附加禁止 `redb`、`nyanpasu-core-manager`，及与 contracts、transform 互不依赖）；`IMPLEMENTATION` 加 profile-io；`STATIC_GATE_PREFIXES` 加两个 `src/`。

**验证**

- 随文件移动的 adapter 测试名清单在迁移前后一致且通过；`evidence_collision_never_overwrites_either_copy` 在 persistence 下通过。
- 单独 `cargo test -p nyanpasu-platform` 与 `cargo test -p clash-nyanpasu --no-run` 通过（platform 测试装配与 tauri 六处测试使用者在本步可编译）。
- 门禁通过；反向检查：给 persistence 临时开 `nyanpasu-utils` 的 `redb` feature，门禁失败。

## A5c — `nyanpasu-net`（整条 HTTP 策略路径，一个提交）

- [ ] 新建 `backend/nyanpasu-net`：订阅抓取拆为 `SubscriptionHttpFetcher` 实现 `SubscriptionFetcher`，字段为 self proxy 端口源、超时、元数据、`SystemProxySource`、四个设备头值；连同 `P/profiles/subscription.rs` 与 `C/utils/key_value.rs` 移入。代理优先级、默认 UA、重试原样保留。
- [ ] 同一提交移入 `P/changelog_http.rs`、`C/client/direct_egress.rs:35-105` 的实现、`C/net_adapters.rs` 的图标与网络诊断实现、客户端策略 `get_reqwest_client` 与 `NyanpasuReqwestProxyExt`（系统代理经 `SystemProxySource`）。
- [ ] tauri：从 `nyanpasu_core::utils::hwid` 取设备信息，按原逻辑（含 `sanitize_for_header`）准备四个头值；`ProfileFilePorts.fetcher` 改由 net 提供；`T/setup.rs` 构造 net 的其余实现并注入；`T/adapters/app_update.rs:88-90` 改用 net 的策略。
- [ ] core 删除 `C/net_adapters.rs` 与 `utils/{candy,config,net,key_value}.rs` 中已无使用者的部分；`SystemProxySource` 的 core 实现保留到 C2；updater 的独立客户端（`C/updater/instance.rs:78-89`）不动。
- [ ] 订阅相关 adapter 测试随文件进入 net；platform 的测试装配与集成测试同一提交改用 net 的抓取类型（platform 加 net 的 dev-dependency），tauri 的六处测试使用者随之可编译。
- [ ] 登记：`IMPLEMENTATION` 加 net；`STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-net/src/`。

**验证**

- `rg 'NyanpasuReqwestProxyExt|get_reqwest_client' backend/nyanpasu-core backend/tauri backend/nyanpasu-platform` 退出码 1。
- 新增确定性测试：给定夹具设备信息，订阅请求的 `x-hwid`、`x-device-os`、`x-ver-os`、`x-device-model` 精确等于期望值（含净化后的值）；`fetch_sends_default_user_agent_and_hwid` 通过；订阅 adapter 测试名清单迁移前后一致。
- 单独 `cargo test -p nyanpasu-platform` 与 `cargo test -p clash-nyanpasu --no-run` 通过。
- 门禁通过；反向检查：给 profile-io 临时加 net 依赖，门禁失败；A4b 的替身测试仍通过。

## A5d — 迁出 platform 的测试并退役 platform

- [ ] 应用集成测试 `P/profiles/fs_service/integration_tests/{profiles,facade,workflow}.rs` 移入 core 测试，core dev-dependency profile-io 与 net，真实适配器在测试本地组装；`P/profiles/test_support.rs` 的组装职责移到 tauri 测试本地支撑，`T/unified_rpc.rs:486,875,964,1087,1151`、`T/adapters/hotkey/tests.rs:27` 改用它。
- [ ] 确认 platform 已无内容，删除 crate 目录、workspace 项、`backend/tauri/Cargo.toml:42,268`。
- [ ] 门禁：删除 platform 及"platform → core 允许"用例。
- [ ] 文档：`docs/development/backend-packages.md`、`docs/development/architecture.md`、`AGENTS.md` §5、路线图的 Delivered baseline、Known gaps、Phase A 状态，按实际结果更新。
- [ ] 按 design §6 复测编译时间，写进 PR 描述；据数据裁定 O-3。

**验证**

- 三组集成测试的测试名清单迁移前后一致且通过。
- `cargo metadata --no-deps --format-version 1` 不含 `nyanpasu-platform`；`rg 'nyanpasu[-_]platform' backend scripts docs/development AGENTS.md` 退出码 1。
- 全量：`cargo clippy --workspace --all-targets --all-features`、`cargo test --workspace`、`pnpm run lint:backend-boundaries`、`pnpm run lint:architecture-ledger`、`pnpm run test:scripts`、三个生成文件无 diff。

---

## Phase B–E 里程碑卡

每张卡开工前先细化为与 Phase A 同粒度的任务并审阅。依赖图、决策、前置、不变量、完成判据见 design §5。

| 卡  | 内容                                                                                          | 前置           | 完成判据（摘要）                                                        |
| --- | --------------------------------------------------------------------------------------------- | -------------- | ----------------------------------------------------------------------- |
| A2b | 脚本扩展注入与引擎 feature                                                                    | A2             | design §5.0                                                             |
| B1a | persistence 收 format 与通用原子写；`session_state` 不依赖 migration                          | A5b            | 无 `session_state → migration`；两个恢复测试通过                        |
| B1b | backup/migration 的存储端口；migration crate                                                  | B1a            | migration crate 不依赖 redb                                             |
| B2  | facade 签名去具体存储类型；traffic legacy 导入器 feature 化                                   | B1b            | `ClientSetupArgs` 类型闭包无 `Storage`/redb；traffic 无默认 redb        |
| B3  | logs、traffic、geo 成 owner crate；`owner_shutdown` 按需建 crate                              | B2             | owner 生产代码不含 clash/core-control/redb/notify；traffic 回归测试通过 |
| B4  | updater crate；后端与其 HTTP 客户端由 shell 注入                                              | A              | facade 不构造 `HttpUpdaterBackend`；facade 无归档依赖                   |
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

# 独立提取 nyanpasu-paths

## 状态与基线

- 分支：`refactor/extract-paths`。
- 起始基线及原测试基线：`dc2a1e630f19c3a1a860bd195c16c048f4ee768a`。
- 交付前将未推送的单一提交衔接到最新 `origin/main`，`019cfd300de8a723ef11408013bdf2d65d3cbee9`；保留 main 新增的日志 4096 行缓冲限制和共享 async runtime，不改变现有 core stack。
- 新 PR 的 base 为 `main`，不属于现有 core 提取 stack。
- 已实施独立 crate 和 GUI 接入，用户已审核修正并明确授权提交、推送及创建 PR。此文档记录提交前的验收；远端交付与 CI 状态以对应 PR 为准，不预先标记通过或合并。
- 已按用户要求丢弃原 Phase 02 的全部 staged／unstaged 改动及两份未跟踪文档；原提交和 gitignored 构建资源未改。仓库外留存备份 `/tmp/nyanpasu-discard-02a-x8xl9hxp`，不作为新实现来源。

开发要求以 [development guides](../development/README.md)、[架构规范](../development/architecture.md)、[RPC](../development/rpc.md) 和 [测试规范](../development/testing.md) 为准。

## 目标与边界

在 `backend/nyanpasu-paths` 建立独立 crate，重新设计 `PathResolver`，不是把旧模块换个位置。Tauri、后续 core 和 CLI 均能消费它，不要求 Tauri、窗口、widget、前端 assets 或假资源。

实例持有目录身份、根目录输入、安装／binary 位置和必要的平台信息。平台差异由 crate 的实例内部和平台实现处理；GUI 只负责真实宿主信息的装配，不在调用链散布 Windows／Unix 判断。

- 共享：目录解析／派生、OS 默认目录、Windows registry／SID／单实例标识、必要的文件系统机制、binary 定位，以及相关路径输入的跨平台处理。
- GUI：Tauri 包资源／executable 探测、应用身份供应、开发 sidecar 的源码树／target-triple 命名、窗口／tray／widget 及 dialogs。
- service 的路径相关输入准备通过实例消费；服务协议、命令模板、提权执行及 actor 生命周期不因路径提取而搬进通用路径库。不能复制两份业务参数模板。
- 不带入 RuntimeBuilder、connection rates、service compatibility 等 Phase 01 提取；不开始 version/device、updater、effects、transactions 或 facade 的其他迁移。
- 不修改现有 PR／推送分支历史。后续 core 提取 PR 的 rebase 是后续工作。

## 实例与平台处理要求

1. 使用共同的实例接口，实际需要的信息挂在实例内；不为每个函数建一个上下文／路径包装对象。
2. Windows／Unix 实现可以分开；平台选择留在 crate 内，必要的宿主专有输入只在实例装配处处理。
3. 不在函数参数或调用实参中插入 `#[cfg]`，不以 `let _`、`_paths`、`_binaries` 或 `allow(unused)` 假装消费多余依赖。
4. 不新增全局 service singleton、万能 locator、GUI 反向回调、仅用于取路径的 `NyanpasuClient` State 或 facade 路径供应接口。
5. 原 `with_base_dirs` 一类明确输入的构造仍可脱离宿主；没有 installation/resources 时不能要求假值。构造、目录发现与目录创建的职责分别表达。
6. 不把错误及 IO 提前到实例构造以求简化：保持独立根目录失败、原操作顺序、best-effort 与传播边界。
7. 共享应用 graph 的输入和具体 GUI 操作是否使用固定启动根目录，是不同约束；不借提取强制 GUI adapters 增加路径字段。

## 从 main 重新核实的现有实现

| 文件                                                                                  | 必须处理的耦合                                                                                     |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `backend/tauri/src/utils/path.rs`                                                     | 存储 config/data/resources，但 from_env、install、binary 和 instance 仍回调 GUI dirs               |
| `backend/tauri/src/utils/dirs.rs`                                                     | OS 默认值、portable／registry、GUI 身份、文件名、FS 副作用及展示路径混合                           |
| `backend/tauri/src/utils/winreg.rs`                                                   | registry namespace 依赖 GUI，SOFTWARE_KEY 为 Lazy leaked string；SID fallback 反查 GUI config root |
| `backend/tauri/src/core/clash/mod.rs`                                                 | binary 优先级与 GUI target-triple／source-tree fallback 混合                                       |
| `backend/tauri/src/core/service/control.rs`                                           | 路径相关准备与共享服务参数／执行混合，必须保留用户解析和目录错误顺序                               |
| early startup、migration、runtime、profile/script、backup、日志／cache、GUI consumers | 按实际职责接入实例，不机械逐层传参或注入宽泛 State                                                 |

全量调用扫描保存在仓库外 `/tmp/nyanpasu-paths-main-callers.txt`；实施时重新扫描，不能把旧 stack 的调用图当成 main 的现状。

## 必须保留的行为

- Windows config：portable → registry → OS default；data 不走 registry override。
- 单实例检查 best-effort，未获得锁就退出的进程不提前创建 data root。
- managed binary 搜索：data → install → GUI 提供的 dev candidate；保留目录准备失败跳过与 exists 策略，不缓存已找到的 binary。
- diagnostics locator 与 managed search 不合并：data 命中仍返回目录，install fallback 仍返回未检查存在性的文件路径。本 PR 不修这一既有差异。
- 日志导出先打开输出 writer／reopen tempfile，再处理目录；HTTP body 保持 tempfile 的持有和清理时机。
- service 准备保持用户 → data → config → install 的顺序、OsString、错误 variant／包装和 shell quoting。
- Serde／Specta／RPC 能力、owner、错误和事件契约不变；bindings 由原导出流程生成。
- 原测试及 fixtures 跟随职责移动，不新增迁移测试，不清理既有未使用代码。

## 执行计划与验收

- [x] 丢弃旧改动，确认 main 独立分支、干净工作树及最新规范。
- [x] 读取 main 的原路径／目录／registry／binary 实现，重新扫描消费者。
- [x] 记录 main 的相关原测试基线。
- [x] 定稿实例输入和方法边界，避免旧 PathResolver 的机械搬迁。
- [x] 建立 crate／workspace 接入及内部平台实现，保持无 Tauri 依赖。
- [x] 同一变更更新全部调用者，删除替代实现和 module 声明，不留兼容 re-export。
- [x] 移动原测试；运行新 crate 和受影响的原测试、Specta export、Clippy、Rustfmt、架构 gate；检查 normal dependency tree。
- [x] 审核 main-base diff：不含旧 stack 提交、不含条件参数／实参或消警告 bypass，未引入 GUI 平台逻辑扩散。
- [x] 用户审核修正并明确确认可以提交、推送及创建独立 PR。
- [ ] 进行原子提交、push 新分支和创建 base 为 main 的独立 PR，远端状态由 PR 记录。Linux 本地结果、未验证平台及 CI 状态分别记录。

## 实施结果

- `PathResolver` 持有 `HostInputs`、平台派生的目录 namespace、可选显式根目录和 installation override；构造本身无目录 IO。
- 原 application graph 改用 `ResolvedPaths` 固定根目录值快照，其内部保留同一配置的 resolver；不是另一个发现服务或每操作一个上下文。
- Windows／Unix 默认目录、registry、SID、单实例、service user／目录准备在 crate 内实现；GUI 的 `host_paths` 仅装配包身份、executable、portable 标记和 dev sidecars。
- service 协议和 OsString 参数模板、GUI tray 图像路径、Tauri self-update 留在 GUI。没有给 widget／tray adapter 添加路径字段，也没有增加仅取路径的 client／RPC State。
- 删除旧 path／winreg／winreg_test 模块和 core binary 查找实现／re-export。原 dirs 文件只保留 GUI version metadata 及原未使用常量；未开始 version/device 提取。
- 按用户审核意见删除全部 `legacy` 实现、导出、仅供它使用的 development 输入及 dirs／chrono／nix 直接依赖。原测试函数名称及数量跨 GUI／新 crate 完整保持；原包含 tray 路径断言的 config-derived 测试随 GUI icon layout 留在 tray 模块。

## 用户审核修正

- README 段落和列表项保持一行，不手动折行；删除 legacy 描述。
- `profiles.rs` 的 `ResolvedPaths` 和 `RefreshOrigin` 测试 import 放入 `mod tests`；模块外的原 test-support 方法使用明确限定的类型名。
- `FsRuntimeBuildAdapter` 在生产与测试使用相同的 `core_specs` 依赖和同一方法。生产注入捕获真实 resolver 的查找行为，原测试显式注入 fake 实现；不再条件化 adapter 字段或方法，也没有未被测试消费的路径参数。
- 之前一次未经用户审核的 commit 尝试被 hook 拒绝，没有产生提交；没有 push／PR 操作。审核修正期间未再 stage、commit 或操作远端，保留了用户审核的 index；用户随后明确批准交付。
- 本轮验证：新 crate 原测试 4 通过／1 原 ignored；client 原测试 575 通过；Specta export 1 通过且 bindings 无 diff；Linux crate／GUI all-targets、all-features Clippy 通过；新 crate Windows GNU all-targets check／Clippy 通过；Rustfmt、README／计划 Prettier、diff、依赖树、架构 gate、45 个 ledger 原测试及 Deno lint 通过。没有新 crate warnings；GUI／依赖既有 warnings 保留。
- 移除 legacy 后，Windows 专用的 `host()` helper 随平台实现放入 `windows.rs`，不留 Linux 下的 dead code。没有新增测试函数。
- 本轮日志：`/tmp/paths-review-correction.log`、`/tmp/paths-review-correction-gates.log`、`/tmp/paths-review-correction-tree.txt`。下表仅记录修正前的全量 suite，不冒充本轮全量复验；GUI Windows／macOS 限制未变。

## 交付前最新 main 衔接验证

在 `019cfd300` 基线上重新验证已审核实现及合并衔接：workspace all-targets／all-features Clippy 通过；GUI lib 串行全量 1187 通过、5 原 ignored、1 明确 filtered（仅排除下述已复现的容器硬件测试）；新 crate 4 通过／1 原 ignored；Rustfmt、架构 gate、45 个 ledger 原测试及 Deno lint 通过。Specta export 随全量 suite 通过，bindings 无 diff。结果记录在 `/tmp/paths-approved-rebase-validation.log`，没有新增测试或改变排除范围。

## 修正前的本地验证与限制

| 检查                                                                     | 结果                                                                |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| 本次 main 基线：path／service control／migration／local host             | 5／2／118／3 通过                                                   |
| 新 crate 原测试（Linux）                                                 | 4 通过、1 原 ignored；另有 2 个原 Windows 测试，仅编译检查          |
| GUI lib 全量测试，`GSETTINGS_BACKEND=memory`、串行，排除已复现的硬件测试 | 1187 通过、5 原 ignored、1 明确 filtered                            |
| GUI lib 首次全量                                                         | 1187 通过、1 硬件测试失败、5 ignored                                |
| 新 crate／GUI all-targets、all-features Clippy                           | 通过；保留既有 GUI／依赖 warnings                                   |
| 新 crate Windows GNU all-targets check／Clippy                           | 通过，无新 crate warnings；不是 Windows runtime 验证                |
| Specta export                                                            | GUI suite 中通过，生成 bindings 无 diff                             |
| Architecture gate／原 ledger 测试／Deno lint                             | 通过；37 allowlisted statics，45 原 ledger 测试；snapshot／阈值未改 |
| Rustfmt／diff／依赖树／测试函数库存                                      | 通过；新 crate normal tree 无 Tauri／egui；无新增测试函数           |

容器硬件型号为空导致 `utils::hwid::tests::test_device_model_not_empty` 失败。同一 main 基线二进制精确复现（exit 101），未修改硬件代码／测试。一次并行全量复验还出现原端口测试固定端口 48332 不可用；串行复验通过，未改变端口测试或 allocator。

`GSETTINGS_BACKEND=memory` 仅作用于测试进程，未更改桌面代理设置。GUI 的 Windows GNU cross-check 在 `aws-lc-sys` 因缺 `x86_64-w64-mingw32-gcc` 被阻塞；不能据新 crate 的成功推断整个 Windows GUI 构建成功。Windows／macOS runtime 和平台 CI 尚未验证。

本次日志位于 `/tmp/paths-main-baseline.log`、`/tmp/paths-main-hwid-baseline.log`、`/tmp/paths-serial-final.log`、`/tmp/paths-crate-final.log`、`/tmp/paths-gui-windows-check.log` 和 `/tmp/paths-gates.log`。旧方案的日志没有用作本次结果。

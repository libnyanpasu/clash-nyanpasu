# 开源许可致谢页实施计划

**日期：** 2026-10-21

**状态：** 已实施（分支 `feat/open-source-credits`），待提交与 CI；实施中对计划的修正见“实施记录”。

**基线：** `main@51ae3ef58`。

## 目标和边界

在“设置 → 关于”中新增“开源许可”页面，列出应用实际分发的第三方组件及其许可证全文。数据有三个来源，构建时合并成一份 JSON：

1. **Cargo**：用 `cargo-about` 扫描 `clash-nyanpasu`（`backend/tauri`）的依赖图；
2. **npm**：用 `pnpm licenses list` 扫描前端生产依赖；
3. **手动组件**：仓库内维护的清单，登记 sidecar、resources、数据文件等不经包管理器、由脚本下载并随包分发的组件。同一文件也用于修正扫描结果中许可证缺失或错误的 npm 包。

本次不涉及以下内容：

- 不新增 RPC，也不改动 Rust 应用代码。数据是构建期静态产物，桌面端和 HTTP Web UI 共用同一前端包。
- 不做许可证翻译。
- 不覆盖安装器工具链（NSIS、WiX 插件），只覆盖应用运行时随包分发的内容。
- 不给 `nyanpasu-widget` 和 `nyanpasu-service` 单独出清单：widget 以库的形式链接进主程序，其依赖已在主程序的依赖图中；service 是独立下载的 sidecar，作为手动组件登记。

## 已核实的现状

| 事实                                                                                                                                                                                                                       | 来源                                                             |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| 仓库中没有任何许可证工具或配置（`cargo-about`、`cargo-deny`、前端 license 插件）                                                                                                                                           | 全仓检索                                                         |
| backend workspace 共有 1288 个第三方 crate（含 dev/build 依赖，以及仅在某些平台编译的依赖）                                                                                                                                | `cargo metadata`                                                 |
| 其中 14 个 Slint crate 的许可为 `GPL-3.0-only OR LicenseRef-Slint-*`；`nyanpasu-ipc` 和 git 依赖 `tauri-specta-query` 缺少 license 字段                                                                                    | `cargo metadata`                                                 |
| `pnpm --filter "@nyanpasu/nyanpasu..." licenses list --prod --json` 共列出 456 个包：包含 workspace 包的依赖（如 `radix-ui`），不含 devDependencies（如 `vite`），但会包含声明在生产依赖里的构建期工具（如 `tailwindcss`） | 本机实测                                                         |
| 上述 npm 结果中有 2 个包的许可证为 `Unknown`：`fast-shallow-equal@1.0.0`、`react-universal-interface@0.6.2`                                                                                                                | 本机实测                                                         |
| `cargo-about` 最新版本为 0.9.2，提供多平台预编译包；`taiki-e/install-action` 支持安装它                                                                                                                                    | GitHub Releases API、install-action `manifests/cargo-about.json` |
| `cargo about generate --format json` 输出 `{ overview, licenses, crates }`：`licenses[]` 每项含 `id`、`text`、`used_by[].crate`，`crates[]` 含完整的 cargo `Package` 和最终选定的 license                                  | cargo-about 0.9.2 源码 `src/generate.rs`                         |
| `about.toml` 中的 `accepted` 是按优先级排列的许可证白名单（不在名单内即报错）；`targets` 按平台过滤依赖；`ignore-build-dependencies` / `ignore-dev-dependencies` 排除非运行时依赖；`clarify` 可手工修正单个 crate 的许可证 | cargo-about 官方文档 `cli/generate/config.md`                    |
| 本地路径依赖（workspace 成员、runtime 子模块中的 crate）仍会出现在输出中并参与许可证检查（`--include-local` 只影响许可文件的收集）；生成器按 `source == null` 过滤掉它们                                                   | 实测 cargo-about 0.9.2                                           |
| 随包分发的 sidecar 由 `tauri.conf.json` 的 `externalBin` 声明（8 项）；`resources/` 下的文件由 `scripts/src/prepare/check.ts` 下载                                                                                         | 仓库代码                                                         |
| `beforeBuildCommand` 只在 `tauri.conf.json` 中定义；`tauri.nightly.conf.json` 由 `prepare:nightly` 合并生成，不入库                                                                                                        | 仓库配置                                                         |
| 关于页 `settings/about/route.tsx` 目前直接渲染页面内容；`settings/debug/` 已采用 `route.tsx`（放 Outlet）+ `index.tsx`（放页面）的嵌套写法                                                                                 | 仓库代码                                                         |
| `@tanstack/react-virtual` 已在日志、规则、连接页使用                                                                                                                                                                       | 仓库代码                                                         |

## 设计

### 数据流

```text
credits/about.toml ─> cargo about generate --format json ─┐
pnpm licenses list --prod --json ─────────────────────────┼─> deno task generate:credits
credits/components.toml + credits/licenses/*.txt ─────────┘           │
                                                                      v
                  frontend/nyanpasu/src/assets/credits/open-source-credits.json（不入库）
                                                                      │  import.meta.glob 懒加载为独立 chunk
                                                                      v
                                                     /main/settings/about/credits
```

### 手动组件清单

清单位于 `credits/components.toml`，非标准的许可证全文放在 `credits/licenses/`。用 TOML 是因为它支持注释，便于写明来源和核实依据：

```toml
[[component]]
name = 'mihomo'  # 唯一
kind = 'sidecar'  # sidecar | resource | data
homepage = 'https://github.com/MetaCubeX/mihomo'
license = 'GPL-3.0-only'  # SPDX 表达式
# 可选：许可证全文文件，路径相对于 credits/；省略时使用 SPDX 标准文本
license-files = ['licenses/mihomo-LICENSE.txt']
# 可选：本条目覆盖的 externalBin 项，供覆盖率检查使用
bundles = ['sidecar/mihomo', 'sidecar/mihomo-alpha']
# 可选：仅在这些平台分发时填写
platforms = ['windows']
notes = '依据 Meta 分支的 LICENSE 文件'

# 修正扫描结果中许可证缺失或错误的 npm 包
[[npm-override]]
name = 'fast-shallow-equal'
version = '1.0.0'
license = 'Unlicense'
notes = 'package.json declares no license; its LICENSE file is the Unlicense.'
```

Cargo 包的修正统一放在 `about.toml` 的 `clarify` 中（cargo-about 原生支持，并用校验和防止上游许可证变更后修正失效），手动清单里不再重复一套 Cargo 覆盖机制。

生成器的校验规则如下，任一项不满足即失败，并在报错中指明文件和条目：

- `name` 唯一；字段与取值不在 schema 内即报错；
- 无 `license` 的组件只声明来源地址，不得带 `license-files`；有 `license` 时须为合法 SPDX，且全文来自 `license-files` 或 SPDX 标准文本（`LicenseRef-*` 必须给文件）；
- `license-files` 所指的文件存在；
- `npm-override` 必须对应扫描结果中实际存在的包和版本，避免依赖升级后留下失效的覆盖条目；
- `tauri.conf.json` 的每个 `externalBin` 项都至少被一个组件的 `bundles` 覆盖（见决策项 D3）。

### 许可证策略

`credits/about.toml` 中的 `accepted` 是全仓库唯一的许可证白名单，npm 扫描结果也按同一份名单检查；手动组件是逐条人工登记的，不受白名单约束。名单中把 `GPL-3.0-only` 排在 `LicenseRef-Slint-*` 之前，使 Slint 解析为 GPL 选项。其余配置：

- `targets`：六个发布目标（Windows x64/arm64、Linux amd64/aarch64、macOS x64/arm64），取各平台依赖的并集；
- 启用 `ignore-build-dependencies` 和 `ignore-dev-dependencies`；
- feature 取 `default` 加 `nightly` 的并集，以覆盖 release 和 nightly 两种构建；不含 `trace-chrome`、`dhat-heap` 等仅用于性能分析、不随包分发的 feature；
- 缺少 license 字段的第三方 crate 在 `clarify` 中写明并附校验和（目前只有 git 依赖 `tauri-specta-query`）。

依赖升级若引入白名单之外的许可证，`generate:credits` 和 CI 都会失败，由维护者决定是否接受。

### 生成产物

```ts
interface OpenSourceCredits {
  schemaVersion: 1
  // 按“规范化文本的 SHA-256”去重的许可证全文
  texts: Record<string, string>
  packages: Array<{
    source: 'cargo' | 'npm' | 'bundled'
    name: string
    version?: string
    homepage?: string
    license?: string // SPDX 表达式；无许可证的随包组件省略
    textIds: string[] // 引用 texts；含该包的 NOTICE 文件
    kind?: 'sidecar' | 'resource' | 'data'
    platforms?: ('windows' | 'linux' | 'macos')[]
    notes?: string
  }>
}
```

- **确定性**：输出按 `source`、名称、版本排序，不写生成时间，同一输入的输出逐字节一致。
- **npm 许可证全文**：读取 `paths[]` 目录中的 `LICENSE*`、`LICENCE*`、`COPYING*`；找不到时退回 `spdx-license-list`（CC0，Deno 中按 `npm:` 固定版本引用）提供的标准文本。双许可（`A OR B`）按 `accepted` 的优先级选取，与 cargo-about 的行为一致。
- **NOTICE 文件**：cargo 和 npm 包目录中的 `NOTICE*` 文件一并收集。Apache-2.0 要求随分发附带 NOTICE，而 cargo-about 的输出不包含它。
- 实测 1538 个条目、459 段去重文本，JSON 1.65 MB（gzip 187 KB），构建后是独立的懒加载 chunk，credits 路由本身的 chunk 为 12 KB。

### 前端

- `settings/about/route.tsx` 改为只渲染 `AnimatedOutletPreset`（设置页的 outlet 按 about 路由切换动画，about 内部的子页切换需要它自己的 outlet）；现有页面内容移到 `about/index.tsx`；新页面放在 `about/credits/index.tsx`，仅它使用的模块放在 `about/credits/_modules/`。
- 关于页增加一个入口卡片（不加载数据，因此不显示数量），点击后用 router link 跳转到新页面。关于页的卡片改用 CSS columns 砖墙布局（与流量总览页相同），各列独立堆叠，不再按行对齐留出空白。
- 新页面包含：搜索（按名称和许可证）、按来源筛选的标签（Rust / npm / 随包组件）、用 `@tanstack/react-virtual` 实现的虚拟列表；列表内每个来源前有分类小标题，数量与筛选标签一样用徽章显示（沿用连接页 `CountBadge` 的样式），随搜索与筛选更新。设置侧栏改为按路由前缀判断选中，进入“关于”的子页面时“关于”仍保持选中。点击条目后在对话框中显示许可证全文与 homepage 链接，外链复用 `settings/_modules/open-external.ts`。
- 数据通过 `import.meta.glob` 懒加载。数据文件不存在时 glob 返回空对象，页面据此显示“此构建未包含许可清单”。开发服务器不运行生成器也能正常启动。
- UI 组件使用 `@nyanpasu/ui` 中已有的控件，并按规范添加 `data-slot`；i18n 键统一用 `settings_about_credits_*` 前缀，6 个语言文件中放在 about 相关键的旁边，随后执行 Paraglide 编译。

### 构建与 CI 集成

- **任务入口**：新增 `deno task generate:credits`，代码放在 `scripts/src/generate/credits/`，按职责拆成 CLI、纯计算（解析、校验、合并）和外部调用（执行 cargo-about、pnpm）三部分。`package.json` 中加一个 `generate:credits` 脚本，只转调该 deno 任务。
- **beforeBuildCommand**：`tauri.conf.json` 改为 `pnpm generate:credits && pnpm run-p web:build generate:git-info && echo $(pwd)`。这样任何 tauri 打包都会先生成清单，缺少 cargo-about 时直接失败，不会悄悄发出没有许可清单的版本。
- **构建工作流**：`deps-build-{linux,macos,windows-nsis}.yaml` 在打包前用 `taiki-e/install-action` 安装 `cargo-about@0.9.2`。
- **ci.yml**：Ubuntu Lint job 增加 `deno task lint:credits`，只校验不写文件，在 frontend、backend、scripts 任一部分有变更时运行。该 job 已经执行过 `cargo fetch` 和 `pnpm install`，renovate 依赖升级引入的新许可证会在 PR 阶段就被发现。
- **其他**：`credits/` 加入 `scripts/src/ci/changed-parts.ts` 的 `CHANGE_RULES`（归入 scripts 部分）并补充对应测试；生成的 JSON 加入 `.gitignore`。

## 决策（2026-10-21 已确认）

| #   | 问题                               | 结论                      | 理由                                                                                                                                                                                      |
| --- | ---------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | 生成产物入库，还是构建时生成？     | **构建时生成，不入库**    | 入库就需要 CI 检查产物是否最新，而 renovate 的每个依赖升级 PR 都会因此失败、需要手动重新生成；构建时生成没有这个问题。代价是本地 `pnpm build` 需要安装 cargo-about（`pnpm dev` 不受影响） |
| D2  | 手动清单放在哪里？                 | **仓库根目录 `credits/`** | 清单同时包含后端 sidecar 和前端数据文件，不专属于任何一个包。备选位置 `backend/tauri/credits/` 离 `tauri.conf.json` 更近                                                                  |
| D3  | 是否检查 `externalBin` 都已登记？  | **检查**                  | 约 20 行代码，可以防止新增内核后忘记登记许可。`resources/` 不做同样的检查，因为本地目录中常有残留文件（本机就有 `clash-verge-service.exe` 等），检查结果会随环境变化                      |
| D4  | 上游没有许可证的随包组件如何处理？ | **仅声明来源地址**        | 这些属于法律判断，不应由我自行决定                                                                                                                                                        |

## 合规待核实

截至 2026-10-21 的核实结果：

| 组件                                 | 下载来源                                | 许可证核实结果                                                                                                               | 结论                                                                 |
| ------------------------------------ | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| mihomo / mihomo-alpha                | `MetaCubeX/mihomo` Releases             | 发布分支 `Meta`、`Alpha` 的 LICENSE 为 GPL-3.0；但 GitHub API 对默认分支 `Main` 识别为 MIT，两者不一致                       | 按发布分支的 LICENSE 登记为 GPL-3.0；需要人工确认                    |
| clash-rs / clash-rs-alpha            | `ibigbug/clash-rs` Releases             | Apache-2.0（GitHub API）                                                                                                     | 登记 LICENSE 与 NOTICE.md；NOTICE 写明带 tuic 构建为 GPL-3.0，待确认 |
| meow / meow-alpha                    | `meow-rs/meow-rs` Releases              | MIT（GitHub API）                                                                                                            | 可登记                                                               |
| ⚠ clash（premium）                   | `zhongfly/Clash-premium-backup`         | 闭源免费软件，备份仓库没有许可证                                                                                             | 按 D4 仅声明来源地址                                                 |
| nyanpasu-service                     | `libnyanpasu/nyanpasu-runtime` Releases | `Cargo.toml` 声明 GPL-3.0，但仓库中没有 LICENSE 文件                                                                         | 用 SPDX 标准文本登记；建议在 runtime 仓库补上 LICENSE                |
| Country.mmdb、geoip.dat、geosite.dat | `MetaCubeX/meta-rules-dat` Releases     | 仓库为 GPL-3.0；README 说明 GeoIP 内容与 `Loyalsoldier/v2ray-rules-dat`（GPL-3.0）一致，而后者使用 MaxMind GeoLite2 数据生成 | 登记为 GPL-3.0，另以无许可证的 data 组件登记 GeoLite2 并写明署名     |
| wintun.dll                           | wintun.net 0.14.1                       | 预编译二进制使用其 ZIP 包内附带的专用许可（官网说明为“不同于 GPL-2.0 的许可”）                                               | 将 ZIP 内的许可文本存入 `credits/licenses/` 后登记                   |
| ⚠ enableLoopback.exe                 | `Kuingsmile/uwp-tool` Releases          | 仓库没有许可证                                                                                                               | 按 D4 仅声明来源地址；自研替代品 #5867 合入后改登记自研版本          |
| Natural Earth 离线地图               | 已打包进前端                            | 公有领域（`assets/maps/README.md`）                                                                                          | 登记为 data 组件                                                     |
| WebView2 bootstrapper                | Windows 安装包内嵌                      | 未核实                                                                                                                       | 属于安装器层面，按“目标和边界”的约定不登记；如需登记请告知           |

## 实施步骤和验收

| 步骤 | 内容                                                                                                                                                                                                       | 估时     | 验证                                                                                                                                                                                            |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1   | 编写 `credits/about.toml`（`accepted`、`targets`、忽略 build/dev 依赖、`clarify`），并试跑 cargo-about                                                                                                     | 0.5–1 天 | 带 `--locked` 运行通过；输出中 crate 数量与 `cargo metadata` 的运行时依赖数量吻合；对照检查 Slint 解析为 GPL-3.0-only、Linux/macOS 专属依赖已出现在结果中                                       |
| T2   | 实现生成器：解析与校验手动清单、标准化 npm 与 cargo 数据、合并去重、收集 NOTICE、执行许可证策略检查；用 Deno 单元测试，测试数据内联在测试中                                                                | 1–1.5 天 | `deno task test:scripts` 中覆盖：每条校验规则的失败用例、双许可的选取、npm 的 SPDX 回退文本、去重、输出的确定性（连续两次生成逐字节相同）、`externalBin` 覆盖检查；Windows 下路径分隔符已规范化 |
| T3   | 构建集成：新增 deno 任务与 `package.json` 转调脚本，修改 `beforeBuildCommand`、`.gitignore`、三个构建工作流、`ci.yml` 的 `lint:credits` 步骤、`CHANGE_RULES` 及其测试                                      | 0.5 天   | `deno task lint:deno`；`check:changed-parts` 的测试；本地执行一次完整的 tauri 打包，确认清单已生成且被打进产物                                                                                  |
| T4   | 填写手动清单与许可文本，按待决策项的结论处理 ⚠ 项，补全 npm 的两个 `Unknown`                                                                                                                               | 0.5–1 天 | `generate:credits` 不再报错；`externalBin` 的 8 项全部被覆盖；每条记录都在 `notes` 中写明核实依据                                                                                               |
| T5   | 前端：拆分 about 路由、实现开源许可页与入口卡片、6 个语言的 i18n、“未生成”状态；把 `about-update-layout.browser.test.tsx` 的导入从 `about/route` 改为 `about/index`；新增 `about-credits.browser.test.tsx` | 1–1.5 天 | 浏览器测试覆盖：列表渲染、搜索、来源分组、许可证全文对话框、数据缺失时的提示；`pnpm typecheck` 需要逐个包加 `--filter` 运行；oxlint、Prettier、stylelint 通过                                   |
| T6   | 整体验收                                                                                                                                                                                                   | 0.5 天   | 记录懒加载 chunk 的实测体积，并确认首屏包没有变大；用 `verge-dev` 构建在真机上冒烟测试（不覆盖已安装的正式版）；HTTP Web UI 中可以打开该页面；CI 三平台全绿                                     |

**合计约 4–5.5 人日。** 主要不确定性在 T1（需要写多少条 `clarify`）和 T4（上游许可证的核实与 D4 的决策）。

### 提交拆分

1. `feat(scripts): generate open source credits`：包含 T1、T2，以及 T4 的清单数据和任务入口。
2. `build: generate credits before packaging`：T3 中的 `beforeBuildCommand` 修改、工作流安装 cargo-about、`lint:credits` 步骤和 `CHANGE_RULES`。这些改动必须在同一个提交里，否则 release 构建会因为缺少 cargo-about 而失败。
3. `feat: show open source credits in about settings`：T5。

## 风险与已知限制

- **npm 结果偏多**：`pnpm licenses list --prod` 会把声明在生产依赖中的构建期工具（如 `tailwindcss`）也列出来。多列不会造成合规问题，暂不处理；如需精确到实际打包的模块，需要从 Vite 构建产物统计，留作后续改进。
- **cargo-about 平台并集偏多**：`targets` 一次性按所有平台求值，可能多列出在任何单一平台都不会编译的 crate（官方文档已说明）。同样属于多列，不影响合规。
- **网络依赖**：cargo-about 的 `clarify` 若引用上游 git 文件，生成时需要联网。CI 有网络；离线开发时 `--offline` 会退回默认文本，此模式只用于本地调试。
- **本地打包门槛**：本地执行 `pnpm build` 需要先安装 cargo-about（`cargo binstall cargo-about` 或从 Release 下载）。已写入 CONTRIBUTING 的构建依赖表。

## 实施记录

在 worktree `.ccg/clash-nyanpasu/open-source-credits` 实施，未准备 Cargo target（本改动不编译 Rust）。

与计划的偏差：

- **本仓库自身的 crate**：cargo-about 会把路径依赖也纳入许可证检查，因此 `accepted` 中保留了它们声明的弃用标识 `GPL-3.0`。runtime 子模块中的 `nyanpasu-ipc` 没有 license 字段，`--fail` 会因此失败，所以不使用 `--fail`，改由生成器拒绝没有许可文本的第三方 crate。
- **白名单新增 4 项，需要复核**：`LGPL-3.0-or-later`（`ranges`，经 `bolt-load`）、`WTFPL`（`terminfo`，经 `deelevate`）、`EPL-2.0`（`elkjs`，经 `mermaid`）、`OFL-1.1`（Inter 字体）。
- **cargo-about 输出**：在 Windows 上它拒绝写入被重定向的 stdout，生成器改用 `--output-file` 写临时文件；`pnpm` 在 Windows 上经 `cmd /c` 调用，以兼容 `.cmd` 垫片。
- **手动组件不受白名单约束**：Wintun 使用 `LicenseRef-Wintun` 加 ZIP 包内的许可文本。

验证结果：

| 检查                                                  | 结果                                                                                                                            |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `deno task generate:credits` / `lint:credits`         | 1538 个条目（随包 10、cargo 1058、npm 470），两次生成逐字节一致                                                                 |
| 生成器单元测试                                        | 18 项通过                                                                                                                       |
| `deno task test:scripts`                              | 除 2 项外通过；这 2 项是 `prepare/resources_test.ts` 在 Git Bash 下被 GNU tar 误解析 `C:`，PowerShell 下 6/6 通过，与本改动无关 |
| 前端全部测试 `pnpm test:frontend`                     | 143 个文件、745 项通过，含新增 4 项 credits 浏览器测试                                                                          |
| 虚拟列表变异验证                                      | 去掉 `scrollMargin` 或其抵扣时，测试均失败                                                                                      |
| `beforeBuildCommand` 经 `cmd /c` 执行                 | 生成清单并构建前端，产物含 credits chunk                                                                                        |
| 类型检查、oxlint、Prettier、`lint:deno`、前端边界检查 | 通过                                                                                                                            |

未完成：真机冒烟（需要完整 Rust 构建）、CI（未推送）。

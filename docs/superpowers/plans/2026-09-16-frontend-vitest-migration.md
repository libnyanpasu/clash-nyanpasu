# 前端 Vitest 测试迁移计划

日期：2026-09-16。状态：迁移已实施并通过本地验证；真实 E2E 仍为 TODO。

检查基线：`f82d8e06175bacc653f4ddefe4e49d98478d9acf`。

## 1. 目标与范围

将前端测试统一到 Vitest，以文件命名自动发现测试，消除每新增一个测试文件就修改根 `package.json` 的维护负担。保留现有回归测试语义，并用正式的模块 mock 和 React 测试环境替代手写 VM harness。

本次范围：

- Node 环境中的纯逻辑单元测试。
- Vitest Browser Mode 中的 React 组件／hook 集成测试，使用 Playwright provider 和 Chromium。
- 测试发现、类型检查、固定命令、CI 和贡献说明。

明确排除：

- 真实 Tauri E2E、桌面驱动、真实 core 或系统代理操作。
- 独立 WebUI 的实现、IPC 解耦和通信协议设计。
- 当前就构建全应用的 IPC mock 服务或浏览器 E2E 基础设施。
- 修改 Rust、Deno 测试框架，或顺带修复无关业务问题。

用户规划：先解耦 IPC，形成真正可独立运行的 WebUI，再通过网页 E2E 验证完整用户流程。本次浏览器测试仍是组件／集成测试，不作为真实 E2E 的替代证明。

## 2. 已核实的现状与基线

- 根 `package.json` 没有 Vitest 依赖，仓库没有 Vitest 配置；这是引入测试设施。
- `test` 为 `run-p test:*`，包含 Rust、Deno 和三个逐文件登记的 Node 测试命令。
- 前端相关 Node 测试实际有五个文件，其中两个未接入总测试入口。

| 当前文件（均在 `scripts/`）    | 用例数 | 接入 `pnpm test` | 本次迁移目标                                |
| ------------------------------ | -----: | ---------------- | ------------------------------------------- |
| `clash-ws-state.test.mjs`      |      4 | 是               | interface 的 Node 单元测试                  |
| `connection-topology.test.mjs` |      8 | 是               | nyanpasu 的 Node 单元测试，保留地图资源检查 |
| `proxy-group-delay.test.mjs`   |      4 | 是               | nyanpasu 的 Node 单元测试                   |
| `log-viewer-state.test.mjs`    |      4 | 否               | interface 的 Node 单元测试                  |
| `proxy-delay-history.test.mjs` |      3 | 否               | interface 的浏览器 hook 集成测试            |

已运行基线：

```sh
node --experimental-vm-modules --test scripts/clash-ws-state.test.mjs scripts/connection-topology.test.mjs scripts/proxy-group-delay.test.mjs scripts/proxy-delay-history.test.mjs scripts/log-viewer-state.test.mjs
```

结果：23 个用例，20 通过、3 失败。三个失败均发生在 `proxy-delay-history` 的 VM 模块链接阶段：mock 依赖表未包含当前源码引入的 `./query-options`。旧 harness 还假定旧版 bindings 导出形状，需要按当前 `queries`／`mutations` API 重建测试边界。该结果不证明延迟历史业务逻辑本身失败。

现有 CI：`.github/workflows/ci.yml` 的 `test_unit` 在 Linux、macOS、Windows 上执行 `pnpm test`，此前会准备 Tauri 资源并构建前端。

## 3. 测试布局与配置决策

采用包内 `tests/` 目录，避免测试进入 `frontend/interface` 和 `frontend/utils` 的 `src` 声明文件构建，也避免放入 nyanpasu 的文件路由扫描目录。

```text
vitest.config.ts
frontend/
  interface/tests/
    clash-ws-state.test.ts
    log-viewer-state.test.ts
    proxy-delay-history.browser.test.tsx
  nyanpasu/tests/
    connection-topology.test.ts
    proxy-group-delay.test.ts
  utils/tests/                         # 有实际用例时再创建
```

根配置使用 `test.projects`，按环境分两个 project：

| Project   | 自动发现规则                                                            | 环境                  |
| --------- | ----------------------------------------------------------------------- | --------------------- |
| `unit`    | `frontend/*/tests/**/*.test.ts`、`**/*.test.tsx`，显式排除 browser 文件 | Node                  |
| `browser` | `frontend/*/tests/**/*.browser.test.ts`、`**/*.browser.test.tsx`        | Chromium，CI headless |

配置要求：

- 两组匹配必须互斥；支持递归子目录，新增用例不改配置或 pnpm scripts。
- 使用独立 Vitest 配置，不直接加载应用完整 `vite.config.ts`，避免单测触发路由、国际化和其他构建插件。
- 测试直接消费源码。仅按实际需要配置 alias、React 转换和浏览器类型；路径以配置文件位置解析，不依赖 shell 当前目录。
- 如果需要 workspace 包 alias，必须同时处理包根和实际使用的子路径；不要让测试误用陈旧的 interface `dist`。
- Vitest、provider 和 React 渲染工具采用实施时经确认兼容当前 Vite、Node、React 的版本，显式声明配置／测试直接引用的依赖并更新 lockfile。
- 测试显式导入 `test`、`expect`、`vi`，不向应用源码添加全局测试类型。
- 单独的测试 tsconfig 检查测试文件、fixture 和配置。Vitest 执行不代替 TypeScript 类型检查；保持包的生产输出不包含测试。
- 不默认加入覆盖率门槛、快照批量更新、重试掩盖失败或空套件成功设置。

## 4. 实施步骤与验收

### T1：建立 Vitest 与类型检查入口

- [x] 按仓库约定创建独立 worktree，独立安装 pnpm 依赖；只在实际需要时准备 UI／Rust 构建资源。
- [x] 添加 Vitest、Browser Mode provider、React 渲染工具及必要的显式依赖。
- [x] 添加根配置、两个 project，以及测试专用 tsconfig。
- [x] 为两个包的测试设置必要的源码解析；React Query／React 实例应正确共享，避免重复 React 导致 hook 错误。

验收：后续 T2、T3 的实际测试均能加载 TS／TSX 源码，且不要求 Tauri 运行、下载 sidecar 或先构建 interface 的 `dist`。不为验证框架另加没有业务意义的示例测试。

### T2：迁移 20 个纯逻辑用例

- [x] 将四个纯逻辑文件移至上述包内目录，改为 TypeScript 和 Vitest 测试 API。
- [x] 保留顺序、去重、上限、失败延迟、循环引用和地图资源等现有断言；迁移到 `expect` 时保持对象身份与深相等断言的区别。
- [x] 使用符合实际类型的 fixture；不以大范围 `any` 或 `@ts-ignore` 绕过迁移错误。
- [x] 修正地图 JSON 的相对路径，以 `import.meta.url` 定位资源。

验收：`vitest run --project unit` 发现并通过原有 20 个用例。测试语义不因迁移而削弱。

### T3：用浏览器 hook 测试替换 VM harness

- [x] 将三个延迟历史用例迁至 `proxy-delay-history.browser.test.tsx`。
- [x] 使用真实 React、`QueryClientProvider`、`QueryClient` 和 `useClashProxies`；可用小型测试组件触发公开操作并展示结果。
- [x] 每个测试独立创建 QueryClient，禁用无关的自动重试；测试结束卸载组件并清理 cache、mock 和未完成任务。
- [x] 在当前 IPC 边界用 `vi.mock` 提供确定性的 query／mutation 结果（实现保留真实 bindings，仅 mock `@tauri-apps/api/core` 的 `invoke`）。保留真实 React Query 缓存和 mutation 生命周期，不再 mock `useMutation` 后直接调用 `onSuccess`。
- [x] 覆盖：响应缺失的节点保留历史；空响应保留全部历史；零延迟作为失败样本保留；原有缓存对象不被原地修改。
- [x] 初始查询与 mutation 返回值应可控，避免 refetch 用旧 fixture 覆盖断言目标。用可观察状态／异步断言等待完成，不使用任意时长 sleep。
- [x] 处理 hook 的 polling timer 与 `onSettled` 清理；不能靠关闭浏览器掩盖泄漏。
- [x] 删除旧 `stripTypeScriptTypes`、`SourceTextModule`、`SyntheticModule` harness 及对应 `.mjs` 文件。

验收：浏览器 project 在无 Tauri 的环境通过三个回归场景；故意破坏一次“保留缺失节点历史”的行为能使测试失败，随后恢复代码。该检查证明测试穿过真实 hook 行为，不只是验证 fixture。

### T4：收敛命令并接入 CI

目标命令：

```json
{
  "test": "run-p test:architecture-ledger test:backend test:frontend",
  "test:frontend": "vitest run",
  "lint:ts:tests": "tsc --noEmit --project tsconfig.test.json"
}
```

- [x] 保留原 Rust 和 Deno 命令，移除三个逐文件 Node 测试命令。
- [x] 总入口使用显式的测试类别列表，避免未来 `test:*` 通配符误启动 watch 或重复执行子命令。
- [x] 本地 watch 使用 `pnpm exec vitest`；单环境执行用 `pnpm test:frontend --project unit`／`--project browser`；单文件使用 Vitest 文件过滤。
- [x] 在现有三平台 CI 的 `pnpm test` 前安装 provider 使用的 Chromium（Linux 同时安装系统依赖），使用 lockfile 对应的 Playwright CLI，不临时下载另一版本 CLI。
- [x] CI 不启用自动更新快照；测试失败保持非零退出码。浏览器下载缓存可后续优化，不作为本次前提。
- [x] 更新前端测试贡献说明，写明命名、运行、浏览器安装、mock 边界与 E2E 尚未启用。

验收：`pnpm test:frontend` 一次执行两个 project 并退出；新的测试文件无需额外登记；已有 `pnpm typecheck` 能包含测试类型检查；总测试入口保留 Rust／Deno 且前端只运行一次。

### T5：最终验证与清理

- [x] 实际发现数量至少包含原 23 个用例／场景，四个纯逻辑文件和一个浏览器文件都被执行。
- [x] 执行前端测试、测试类型检查、受影响文件格式／lint 检查和必要的包构建，确认声明文件输出不含测试。
- [x] 验证 `--project` 过滤和 watch 的增量发现；添加临时测试确认自动发现后移除，不提交框架自测文件。
- [x] 搜索并清除本次迁移范围内的 `node:test`、旧测试路径和逐文件执行命令；历史计划中的记录无需改写。
- [x] 检查 test discovery 不包含 Deno architecture ledger 或生成目录。
- [x] 执行可用的完整 `pnpm test`；如果 Rust／平台环境阻塞，单独记录失败原因，不能将前端通过表述为全仓通过。
- [x] 检查 diff 不含 IPC 重构、业务行为改变、生成 bindings 变化或无关格式化。

依赖顺序：T1 → T2／T3 → T4 → T5。迁移可作为一个完整、可构建的提交；不要提交只接入部分测试、漏掉失败旧用例的中间状态。

## 5. TODO：IPC 解耦后的 WebUI E2E

本节为后续工作，不是本次迁移的验收条件。不添加空 E2E project、占位脚本、桌面驱动或相关依赖。

- [ ] IPC 解耦后，WebUI 能在普通浏览器独立启动；业务调用通过明确的通信适配边界，核心前端逻辑不依赖 Tauri 全局对象。
- [ ] 明确 WebUI 开发／测试启动命令、真实服务连接方式、服务 readiness 和关闭流程。
- [ ] 提供隔离的服务配置与测试数据，具备确定性的初始化和重置能力；避免测试触碰开发者实际代理／订阅配置。
- [ ] 再评估网页 E2E runner，优先评估 Playwright Test。与 Vitest Browser Mode 可以共用 Playwright 生态，但不强求共用 runner 或 fixture 生命周期。
- [ ] 为真实 WebUI + 服务增加少量完整流程：载入配置、修改并重新读取、代理选择、错误反馈；具体流程随 WebUI 能力确定。
- [ ] E2E 覆盖真实通信链；mock 通信的测试继续归为组件／集成测试。
- [ ] 增加固定的 E2E 命令、独立 CI job、失败 trace／截图和服务日志。具体浏览器矩阵在 WebUI 落地时确定。

## 6. 参考

- [Vitest Browser Mode 与多个测试环境](https://vitest.dev/guide/browser/)
- [Vitest Test Projects](https://vitest.dev/guide/projects)

实施时核对所选版本的文档和 peer dependencies；本计划不预先锁定尚未验证的版本号。

## 7. 实施验证记录（2026-09-16）

- 本地 Windows 上完整 `pnpm test` 通过：Vitest 23 个、Deno 33 个、Rust 981 个通过；Rust 既有 2 个用例／doctest 被忽略。
- `pnpm typecheck` 通过，包含新增测试类型检查。
- `pnpm web:build` 和 `pnpm -F @nyanpasu/interface build` 通过；interface 输出不含测试文件。
- 变更文件的 Prettier、Oxlint 和 `git diff --check` 通过；`pnpm install --frozen-lockfile` 通过。
- `vitest list --json` 确认仅收集五个前端文件的 23 个用例，Node 与 browser project 无重复。
- watch 已验证自动发现新增测试，临时测试随后移除。
- 临时破坏缺失节点历史保留逻辑后，两个浏览器回归用例如预期失败；源码恢复后全部通过。
- 首次完整测试前补齐了 worktree 的递归子模块、资源链接与构建产物；未修改对应版本或生产源码。
- CI 添加三平台 Chromium 安装步骤；Linux／macOS 执行结果以 PR CI 为准。

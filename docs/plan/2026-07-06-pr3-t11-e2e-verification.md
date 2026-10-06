# PR-3 T11 — 端到端验证 + 文档收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** design §16 判据 1–8 逐条取证留痕;真实旧数据 e2e(迁移→启动→激活→生成→核心可运行);两项 plan 期挂账风险复核(secret 一致性、advice BC);迁移账本与 guide/roadmap 文档收尾;PR 描述就绪。

**Architecture:** 本卡不写业务代码——只有验证脚本/清单与文档。e2e 用隔离配置目录(`PathResolver::from_env` 的环境变量覆盖,执行时 grep `path.rs` 确认变量名;若无覆盖机制则用便携模式目录)驱动 debug 构建,legacy 样本复用 T02 迁移测试 fixtures 的旧 schema 形态。人工步骤(前端冒烟)以显式 checklist 交付,可由执行者或用户逐项勾选。

## Global Constraints

- 一切 cargo 操作前设 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`。
- e2e 需真实 sidecar/resources(worktree 已 symlink 主检出,CLAUDE.md §17)。
- 判据证据全部落 PR 描述(命令 + 输出摘录),不许"我看过了"式断言(verification-before-completion)。

## 基线事实(2026-07-06 实测)

- 判据原文:design §16:585-592(1 grep 零残留;2 迁移 e2e;3 thin adapter + 纯度;4 读写 call 断言 + 写 handler 无网络;5 引用保护/ProfileHasNoFile/validate 测试;6 golden;7 台账;8 构建 + 前端冒烟)。
- 双迁移机制:`lib.rs:119-133` `init::run_pending_migrations()`(单实例后、UI 前)+ `setup.rs:22-26` `core::migration::Runner.run_pending()`(client 构造前)。rev3 迁移器注册于 `core/migration/registry.rs:4-11`(`modules::profiles::MIGRATOR`),`detect_baseline` 遇 legacy 文件返 0(modules/profiles.rs:24-37)→ **setup.rs 路径必然生效**;lib.rs 路径是否也含 profiles 模块执行时确认(e2e 观测哪条先写 `.bak` 即为准)。
- `.bak`:CLEAN_SCHEMA step 写 `profiles.yaml.bak`(design §10 安全行)。
- 台账预期(T07 勘误后):新增 TODO/FIXME(actor-migration) 注释块 7 处(regenerate draft/LegacyCoreBridge×2/桥定义/resolve_setup 回写/enhance dead_code[T10 已随删除消亡→实际存活 6 处,执行时以 T10 后现场为准]/update_config legacy 路径);判据 7 的「仅两处」原文已被勘误取代。
- plan 期挂账风险:①**secret 一致性**——typed `overrides.secret`(uuid,入生成配置)与 legacy IClashTemp secret(api 客户端读)未强制同步(T07 未回写,字段私有);症状 = 核心起但 api 全 401/连接面板空。②**advice BC**——前端 advice 面从 legacy 分析建议变为 executor 阶段日志。
- bindings.ts 已由 T09 首步导出;前端构建绿是 T09 出口判据,本卡复跑留痕。

## 契约修正(执行后回写 task.md T11 卡,§5.3)

1. e2e 实测记录:rev3 实际生效的迁移路径(lib.rs 子进程 vs setup.rs in-process)与 `.bak` 落点。
2. 判据 7 台账以 T07/T10 勘误后的注释块清单为准(数目 + 逐处枚举),roadmap §5 B8 行同步。
3. secret 一致性检查结果:若 401 复现,修复(最小:resolve_setup 回写 secret 至 IClashTemp,需 nyanpasu-config 暴露 getter——跨 crate 小改,单独 commit)记录于此。

---

### Task 1: e2e 冒烟(判据 2)

- [ ] **Step 1: 搭隔离数据目录**

Run: `Select-String -Path backend/tauri/src/utils/path.rs -Pattern "from_env" -Context 0,15`
记录环境变量覆盖名(如 `NYANPASU_CONFIG_DIR`/便携 flag)。创建 `$env:TEMP\pr3-e2e\config`,放入 legacy 样本:

- `profiles.yaml`:从 `core/migration/modules/profiles.rs` 测试 fixtures 抄一份**含** `current: [a]`、`chain`、remote item(url 任意)与 local item 的旧 schema 文档;对应 `profiles/` 目录放旧物化文件。
- `verge.yaml`/`config.yaml`:最小合法 legacy(或留空走默认)。

- [ ] **Step 2: 启动观测**

Run(worktree 根): `$env:<覆盖变量>='...\pr3-e2e\config'; cargo run --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`(debug;首启同时刷新 bindings 导出属预期)
Expected 逐条勾:

- [ ] `profiles.yaml` 变为 rev3 clean schema;`profiles.yaml.bak` 在位(记录哪条迁移路径先落 —— 日志/时序)
- [ ] 应用窗口起;日志无 panic
- [ ] `clash-config.yaml` 生成且含 guard 注入的 `mixed-port`(值 = SessionPortResolver 解析结果)
- [ ] 核心进程在跑(任务管理器/日志 `run core`)

- [ ] **Step 3: 激活链路**

UI 中切换激活另一 profile → Expected:`clash-config.yaml` 重写、核心 api 收到 put(日志)、连接不中断(`break_when_profile_change` 默认 false)。

- [ ] **Step 4: secret 一致性(plan 期挂账①)**

UI 打开连接/代理面板 → Expected:数据正常(api 200)。若 401/空:按契约修正 3 的最小修复路径处理并单独 commit,证据留痕。

- [ ] **Step 5: advice BC 复核(挂账②)**

打开日志/后处理输出面板 → Expected:显示 executor 阶段日志(scoped/global/builtin 键位正确);记录截图/文本入 PR 描述。

---

### Task 2: 前端全功能冒烟(判据 8,人工 checklist)

在 Task 1 的运行实例上逐项:

- [ ] 导入订阅(真实 URL 或本地 http 服务)→ 列表出现、名称为 url 末段 fallback、内容已物化
- [ ] 新建 Local 配置(File)→ 自动激活判定正确(current 为空时)
- [ ] 激活/切换(单值 current;UI 无多选激活残留)
- [ ] 拖拽重排 + 列表重排
- [ ] 编辑:metadata 改名 / remote options 改 interval / definition 原子替换(三类分开提交)
- [ ] 文件编辑(view/read/save;Remote 只读拒写 toast、Composition 无文件提示)
- [ ] 删除:被引用删除弹 `ProfileInUse` toast(引用保护);普通删除成功
- [ ] 手动更新订阅(update)+ 到期自动刷新(把 interval 调 1 分钟观察一轮)
- [ ] 「多选 File Config → 创建 Composition」最小交互可用(T09 交付)
- [ ] 前端 `pnpm -F interface build && pnpm web:build` 绿(命令留痕)

---

### Task 3: 判据 1/3–7 取证台账

- [ ] **判据 1**(T10 已过,复跑留痕):三条 grep 零命中 + `config/profile/` 不存在。
- [ ] **判据 3**:16 条命令逐条目检 thin(一次 facade 调用);纯度 grep:

```powershell
Select-String -Path backend/tauri/src/state/profiles,backend/tauri/src/client/profiles.rs -Pattern "tauri::|crate::config::Config" -Recurse
```

Expected: 零命中。

- [ ] **判据 4/5**:定点跑 T04/T05 既有测试组(写 `call(_, None)`/读 `call(_, Some)` 断言、下载-提交分离、引用保护、ProfileHasNoFile、validate 拒绝脏落盘)——`cargo test -p clash-nyanpasu state::profiles client::profiles`,记录用例名清单。
- [ ] **判据 6**:`cargo test -p clash-nyanpasu golden_` 4 passed(fixtures 与 T06A 提交时 byte 一致:`git diff --stat` 空)。
- [ ] **判据 7**:

```powershell
Get-ChildItem backend/tauri/src -Recurse -Include *.rs | Select-String -Pattern "(TODO|FIXME)\(actor-migration\)"
```

逐处枚举,与 T07/T10 勘误后的预期清单一致;写入 roadmap §5 B8 行。

- [ ] **判据 8(构建面)**:`cargo build && cargo test` 全绿 + 前端构建绿(Task 2 末项)。

---

### Task 4: 文档收尾 + PR 描述

**Files:**

- Modify: `docs/design/actor-migration-roadmap.md`(§2.1 PR-3 状态行 → 已实施;§5 台账 B8 登记 T07 注释块清单)
- Modify: profiles 迁移 guide(状态行标注「已实施」;文件名以 docs/ 现场为准)
- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T11 卡执行修正块,含契约修正 1–3 实测结果)

- [ ] **Step 1: 文档三处更新**(内容按上表;roadmap B8 行引用判据 7 的枚举)。
- [ ] **Step 2: PR 描述汇编**——判据 1–8 checklist(命令+输出摘录)、BC 清单(命令面 13→16、current 单值、chain→transforms、import 命名、save 参数收紧、advice 面、连接中断挂全 rebuild[用户决策])、台账枚举、评审待办(codex T05/T06 评审延期项)。
- [ ] **Step 3: Commit**

```powershell
git add docs
git commit -m "docs(pr3): finish migration ledger and mark PR-3 implemented"
```

---

## Self-Review 结论

- 覆盖:卡片 4 项内容全落位;判据 1–8 各有取证步骤;plan 期两项挂账风险(secret/advice)有显式复核步骤与修复预案。
- 无占位符:环境变量名/迁移生效路径为「执行时一条命令即得」的现场事实,步骤内给了获取命令;legacy 样本给了来源(T02 fixtures)。
- 一致性:台账数目与 T07(+7)/T10(-1,enhance dead_code 随删除消亡)勘误链一致,判据 7 原文已声明被勘误取代。

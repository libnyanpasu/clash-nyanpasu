# T6–T9 review loop

## 第一轮

先跑全量 lib 回归：784 passed、42 failed、1 ignored；另一个 legacy host 等待测试单独处理。旧测试的 commit-before-apply、缺失 confirmed baseline、源 actor 队列位置等断言需要按 TCC 改写，不能据此削弱生产 baseline 拒绝规则。

实现审查发现并修复：

- retry 成功后派生产物发布失败不能丢失维护状态。记录 pending product，显式重试只发布仍对应最新 receipt 的产物，不重新提交 source/core；用失败开关测试。
- Local host 切换成功后的 daemon stop 必须在 Confirm 后，不能丢掉旧入口的收尾语义，也不能提前破坏 Cancel 可恢复性。
- interruption/publish 的已提交告警保留结构化 code/phase，而不是从 detail 文本解析。
- legacy 转换读取复合字段时，以该次读取的 source version 调用 actor PatchIfVersion；保留命令 hints，并拒绝过期转换，防止覆盖并发新值。
- lifecycle uncertain 即使没有 source mutation recovery context，也不能显示 Healthy 或把 RetryNow 当作成功。

## 后续修复

- legacy Session 入口只保存 main window，保留其他窗口；返回实际 Session source version。
- repair 转换绑定读取时的 source version，actor 再执行 CAS；新增并发 repair 只允许一笔提交测试。
- Profiles 冲突时保留 staging cleanup 失败，避免只报告版本冲突而隐藏待清理资源。
- 删除已经退化为透传的 facade commit_and_reconcile，以及不再使用的 profiles 测试 notifier 参数。
- 前端按序号拒绝旧状态，RetryNow 区分明确拒绝和 transport 未确认；外围 owner 使用用户可读标签。

## 验证过程

- client 第三轮 449 passed、2 failed，剩余为旧测试对 baseline / 重复 activation 计数的断言。
- 全量 lib 后续 827 passed、1 failed、1 ignored；最后一项旧断言改为验证失败前后 baseline 不变。该轮 bindings 导出通过。
- 前端 27 tests / 7 files 通过；TypeScript 四个项目 typecheck 通过；生产构建通过（既有大 bundle 提示）。新增前端代码 oxlint 通过。
- architecture-ledger 33 tests 通过。迁移残留核对后更新稳定快照，gate 通过：Config 全局调用 83 → 49；legacy DTO 引用 320 → 235，新增纯转换引用来自版本约束 repair。
- 最终 `cargo +nightly-2026-09-23 test --manifest-path backend/Cargo.toml --all-features -- --test-threads=8`：**1141 passed、0 failed、2 ignored**。其中应用 lib 830 项通过；ignored 为既有目录 placeholder 和文档示例。
- 最终 `cargo +nightly-2026-09-23 clippy --manifest-path backend/Cargo.toml --all-targets --all-features` 退出 0；存在非阻断 warning，未扩散修改无关代码。
- Rust fmt、改动前端文件 Prettier / oxlint、`git diff --check` 通过。
- all-features 首轮发现 source 失败返回与 Cancel 完成之间的测试时序错误；改为等待 mutation journal 的明确完成通知后断言，最终全量通过。
- 此次 review loop 已完成：实现审查 → 修复 → 专项回归 → workspace 全量验证。
- 日志保存在工作树外：`G:/Programs/Rust/.ccg/clash-nyanpasu/tcc-t6-t9-validation`。review 验证结束时未 commit、push 或合并。

## 验证边界

使用 nightly-2026-09-23，关闭本机 RUSTC_WRAPPER；不修改项目 toolchain。测试使用 fake ports，未操作真实系统代理、安装 daemon 或启动真实核心。早期一次 Windows materialization 临时文件残留断言失败；随后完整 lib 和 all-features 均通过该用例，最终使用同一已编译测试二进制连续 5 次专项验证全部通过。未能再次复现，保留本条环境观察，不声称已定位 Windows 临时文件残留的根因。

## Stacked PR 拆分

- `feat/tcc-config-convergence` → `main`：T6–T8 和 T9 后端/IPC、必要调用方迁移。独立验证 1141 Rust tests 与 25 frontend tests 通过，TypeScript 与架构 gate 通过。
- `feat/tcc-configuration-status` → `feat/tcc-config-convergence`：T9 状态区、transport 未确认提示、翻译与状态顺序测试。
- 先合并底层，再将上层 retarget 到 `main`；仓库 CI 的 base 分支过滤只包含 main/dev/release-*，上层在 retarget 前可能不触发该 CI。

# T2–T5 审计契约修整

审计基线：`feat/tcc-v2-critical-runtime-participant`，`0a7ed9a7e243c9f708efca4bd3f4cda1155c8254`。

本轮范围为 A/B/C 修整，按 v2 Task PR 整理。T6 生产入口换线、Profiles journal 实际接线、T8 自动收敛和 T10 关闭恢复仍是后续工作；本栈不是可独立交付的生产换线结果。

| Task       | 修整契约                                                                                                       | 验收证据                                                                                                   |
| ---------- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| T2 / #5322 | 源事务一次发布带资源结算的终态；必要写入及恢复由源任务持有；取消不能释放在途写入的所有权                       | YAML 失败后补偿 barrier、补偿失败、调用者取消、CAS 冲突、决定唤醒                                          |
| T3 / #5323 | 完整 RuntimeInputs 及目标身份；命令 hints 与目标身份分离                                                       | 三域输入和固定内容改变身份，GUI 字段不改变身份                                                             |
| T4 / #5324 | 固定提交前置条件；提交边界保留未转发/未知证据；ConfirmedRuntime 关联实际运行记录，端口派生；恢复匹配新 binding | actor 内宿主 generation fencing、实例错配、迟到 inspection、候选端口隔离                                   |
| T5 / #5320 | 等待唯一源决定；核对 RestorableBaseline；按完整目标和实际宿主执行；非关键保存跳过 core 查询                    | 跨域 Deferred 后同值重试、同值宿主选择、历史回执错配、基线后 revision 改变、提交前状态查询失败、丢普通通知 |

## 边界

- state 决定源配置是否提交；workflow 不推断或传输另一份决定。
- `Aborted(Restored)` 只在必要本地恢复完成后发布；无法确认的资源结果明确携带 incident。
- core 回执描述实际运行事实，Try 成功即登记；源提交后才发布公开 runtime 文件。
- 历史 receipt 只有与 fresh observation 身份相符时才可成为恢复基线。恢复不要求旧实例复活，但观察必须匹配本次恢复回执。
- 构建消费捕获的 Profiles 内容，不在摘要形成后重新读取文件。外部文件变化作为新的输入处理。
- 自动重试预算不决定 desired/applied 是否相等，GUI 保存不消费预算。
- 原有 facade 生产保存路径尚未换线；A/B/C 通过后才进入 T6，不添加临时 facade 锁。

## 验证

在独立 worktree 使用独立 Cargo target，复用主 checkout 的 sidecar/resources，并为 Rust 构建放置独立 frontendDist 占位。

各层在独立可编译的提交上验证：

- T2：`nyanpasu-core --lib`，118 项通过。
- T3：`clash-nyanpasu --lib client::application_workflow`，98 项通过。
- T4：`clash-nyanpasu --lib`，758 项通过、1 项忽略。
- T5：整栈 `clash-nyanpasu --lib`，800 项通过、1 项忽略。
- 各层提交 hook 执行全目标/全特性 Clippy 和 Rust 格式检查；architecture ledger gate 通过。Clippy 仍有仓库既有警告。

未执行真实 core、GUI 或跨平台 smoke；fake ports 测试不替代这些验证。

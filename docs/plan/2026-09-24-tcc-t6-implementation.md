# T6 三域与受管文件接入

基线：`main@232321d52`（T5 及 review 修复）。规范采用选择性 TCC v2；旧版编号不适用。

## 范围与假设

- 按 T6 → T7 → T8 → T9 顺序逐项计划、实施、验证；全部完成后 review loop。
- T6 迁移状态写入，不提前实现 T8 自动重试或 T9 前端状态展示。
- 源状态仍由各域 actor 持有；workflow 只持有只读快照。
- 运行参与者是已有 actor client；影响分类是纯计算；文件 journal 是注入的 adapter。
- 启动加载不参加运行事务。组合根通过显式启动依赖连接 workflow，未连接时禁止生产写入；独立领域测试注入测试用参与者。

## 实施顺序与验收

1. 三域每笔写入创建唯一 OperationId 和 Required participant，保留 typed hints 与命令策略。验证：保存拒绝不推进源版本，读快照不等待 prepare，同值选择仍执行策略。
2. Profiles 将必要 promote/compensate 纳入 state 的 local_write/local_recovery；固定候选内容随该次 mutation 传递。验证：Try 读新内容，提交前源快照和正式文件不变，发布失败与补偿结果进入权威决定。
3. 受管编辑、刷新、导入与定义替换统一路径；导入和自动激活保留两次事务。移除这些入口的重复提交后 runtime apply。验证：后台写入也经过参与者，无直接受管文件写盘入口，无自身 watcher 回环。
4. 运行领域、workflow、文件 journal 回归及 Rust 格式检查，检查 diff 和架构边界；记录实际结果后进入 T7。

## 环境

独立 worktree：`G:/Programs/Rust/.ccg/clash-nyanpasu/tcc-t6-t9`。
仅 sidecar/resources 链接主 checkout；Cargo target、node_modules 和 frontendDist 独立。

## 验证记录

- 初始化嵌套子模块前基线 Cargo 未启动（缺少 nyanpasu-utils）；已执行递归初始化。
- `pnpm install --frozen-lockfile` 通过。

- Windows 默认 nightly 出现 rustc ICE，改用本机安装的 nightly-2026-09-23，关闭全局 kache wrapper；未修改仓库 toolchain。
- `cargo +nightly-2026-09-23 test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib <filter>`：`client::profiles::tests` 66 通过；`client::application_workflow::tests::mutations` 50 通过；`managed_edit_uses_candidate_bytes` 1 通过；`state::` 13 通过；`service::profile_file::` 46 通过。
- 受管编辑不使用订阅下载的 proxies/proxy-providers 必填规则；活动配置由同一 runtime candidate 构建和检查流程验证。
- 广域 client 回归仍有旧提交后 apply / 同步 GUI / 无 baseline 的 fixture 断言需要随 T7–T9 接口迁移后复核；不宣称全量通过。一次旧 service 用例无限等待被终止，未计为通过。

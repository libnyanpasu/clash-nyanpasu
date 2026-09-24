# T7 外围异步通知与协调

## 范围与分类

T6 三域参与者和资源专项通过后开始本任务。新增 EffectsActor 持有 desired、排队工作与完成状态；已有 SystemProxy/Hotkey actors 保持本地一致性，UI adapters 不逐个包装 actor。完整目标投影与计划合并是纯计算。无新的 Required peripheral participant。

## 实施与验证

1. 组合根构造 typed EffectsClient，workflow 在权威 Committed 后通知。删除 facade 的第二道 commit/effect gate；后台 mutation 同样通知。验证：拒绝无通知、提交回执不等待外围、同值无无谓 dispatch。
2. 固定 proxy/guard、hotkey/autolaunch、visual 三组独立且每组顺序执行；队列按种类合并最新完整 desired，Full 覆盖 Part，locale 在 tray 前。完成状态按目标 revision 隔离。验证：阻塞 PAC 与阻塞 GUI 的独立进度、迟到结果不能覆盖新目标、合并不丢 full refresh。
3. 去掉 GUI 直接进入 TCC Confirm 的调用；同步通知失败不改变 source 结论。executor 分开 accepted 与成功 applied revision。验证：GUI 永久失败不引起 Cancel，失败不会谎报 applied。
4. 更新异步语义回归，运行 effects / UI owner / mutation 子集、编译与格式检查。T8 再添加预算与重试；T9 暴露 IPC 和前端状态。

## 验证记录

- `cargo +nightly-2026-09-23 test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib client::effects::`：42 通过。
- 同命令 `client::ui_effects::`：17 通过；`client::application_workflow::tests::mutations`：50 通过。
- 首轮只有拒绝 mirror 的 fixture 在启动加载阶段提前失败，改为加载完成后启用拒绝；未绕过生产 mutation participant。
- 已删除 facade 第二道 gate 和 retry map。组内任务受 actor 跟踪，完成按 desired revision 过滤；GUI 和 PAC 不参与 source ACK。

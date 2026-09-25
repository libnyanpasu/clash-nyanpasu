# T8 未收敛状态、预算与显式恢复

## 范围与假设

T7 专项验证通过后开始。重试状态归 EffectsActor / ApplicationWorkflowActor 所有，预算和退避是纯策略；依赖探测与 runtime operation 查询由注入的现有 ports 完成。仍禁止任何旧目标自动回写 source。

## 实施与验收

1. 外围目标独立持有 attempts、自动剩余预算、下一次时间与 health；等待依赖不消耗应用预算。初次失败后至多三次自动重试（1/5/30 秒）。相同目标/无关保存不补预算，RetryNow 只允许一次探测。验证：测试时钟推进、永久失败不循环、旧 timer 不应用新目标。
2. runtime deferred 复用 workflow 执行域：重读 committed inputs、核对完整 target identity，再校验和应用；只对已提交目标做收敛，不制造 source 事务。Stopped/依赖未确认进入等待状态，已知永久错误或预算耗尽进入 Blocked。验证：同目标预算保持、stop 无 submit、目标 superseded 不回写。
3. RecoveryRequired 的显式探测先查询原 operation，再验证实际状态；未查询到终态或本地资源未恢复时保持隔离。仅权威源决定与验证成功允许清除未知状态。验证：未知不盲重放、原 operation 终态后按决定恢复。
4. facade 提供带预期源版本的正向修复入口；过期版本拒绝，正常 participant 仍生效。T9 再接 IPC/UI。
5. 运行 targeted actor、mutation/recovery 测试和编译/格式检查；记录真实结果后进入 T9。

## 验证记录

- `cargo +nightly-2026-09-23 test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib client::system_proxy::`：32 通过。
- 同命令 `client::effects::`：46 通过；`client::application_workflow::tests::mutations`：54 通过。
- 测试覆盖自动预算耗尽、手动探测不补预算、相同字段显式重评估、缺少依赖不花预算、Stopped 不启动、未知先查原 endpoint/operation、正向 repair 的过期 source version 拒绝。
- proxy guard 不再绕过预算安装未成功的 desired，也不在 active binding 丢失后重新安装旧端口。
- 本地资源仍未恢复或原操作无法查询到终态时保持 RecoveryRequired，不用重启或盲恢复冒充已验证。

# Windows 内核正常退出与 Meow TUN 热更新

日期：2026-10-22。关联 #5883。依赖 [utils #203](https://github.com/libnyanpasu/nyanpasu-utils/pull/203)、[runtime #457](https://github.com/libnyanpasu/nyanpasu-runtime/pull/457)。

Windows 上关闭 Meow TUN 会重启内核，而 `GracefulKill` 直接终止 Job Object，内核来不及恢复网卡 DNS。本次修复分两步：让停止请求先发 Ctrl+C；单独切换 `tun.enable` 时改用 PATCH，不重启进程。

## Windows 正常退出

```text
CoreManager.stop / retire_current / shutdown
  -> Instance.stop_and_confirm_dead -> Supervisor.stop
  -> ProcessHandle.graceful_kill -> engine Ctrl::GracefulKill
```

内核以 `CREATE_NO_WINDOW` 启动，持有不可见 console，GUI 和服务宿主都没有 console。向它发 Ctrl+C 需要 `AttachConsole`，这会改变调用进程的 console 状态，所以放到一次性 helper 进程里执行：

```text
nyanpasu-service.exe --nyanpasu-process-shutdown <pid>
```

- `nyanpasu-service` 在注册 Ctrl+C 或初始化服务前识别该入口，不另增随包二进制。
- helper 路径由组合根显式注入：GUI 用 setup 已定位的 `service_binary`，服务用自身路径；经 `ManagerOptions.windows_shutdown_helper` 传到 `process::Command`。Unix 忽略该项，仍发 SIGTERM。
- 首次 `GracefulKill` 启动 helper，并沿用原 `kill_grace` 截止时间；重复请求不再发送 Ctrl+C，也不延长截止时间。
- helper 缺失、失败或超时时立即强杀整个 Job Object；内核到期未退出同样强杀。
- PID 复用：引擎在回收子进程前一直持有其句柄，回收后撤销 helper，因此请求只发给仍存活的子进程。

## Meow TUN 热更新

```text
CoreManager.reconcile -> apply_locked -> 分类
  -> Meow 且 source/effective 的唯一变化是 tun.enable 布尔值 -> ConfigChange::Patch
  -> PATCH /configs {"tun":{"enable":<bool>}}
  -> GET /configs，经 RuntimeProjection 比较 tun.enable -> reconcile 健康检查
```

- 只发送 `enable`，不把 Meow 的其他 TUN 字段经 Mihomo DTO 发出。进程规格变化、其他字段或混合变化仍走 Switch。
- Meow 的 `GET /configs` 不符合 Mihomo 的 `RuntimeConfig` 类型，因此读取原始 JSON；比较逻辑与 Mihomo 共用 `RuntimeProjection`，缺少字段视为不匹配。
- PATCH 返回错误也可能已生效，仍以回读为准；回读不匹配或健康检查失败时沿用 `restart_with_compensation`，此时由第一步的正常退出清理旧进程。

## 验证

| 范围                                           | 结果                                                                                          |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------- |
| utils process lib/kill/output/stdin/supervisor | 47 项通过：Ctrl+C 触发清理与 Drop、重复停止只发一次、缺失 helper 立即回退、挂起 helper 被回收 |
| manager `config_apply`                         | 21 项通过，含 Meow 开关、PATCH 报错但已生效、204 无效果、拒绝 PATCH、回读缺字段、健康检查失败 |
| manager 配置分类单元测试                       | 31 项通过                                                                                     |
| clash-api / service main                       | 22 / 4 项通过                                                                                 |
| Clippy、rustfmt                                | 通过，仅余主线已有 warning                                                                    |

utils 的 Ctrl+C 测试需在有 console 的宿主中运行（PowerShell、CI）；在 Git Bash 下 helper 无法附加目标 console。manager lib 中 `native_store::paths` 的两个 Windows 路径分隔符断言在主线上同样失败，与本次无关。

早期实现曾用 SCM / LocalSystem / Session 0 服务宿主、GUI 宿主和真实 CoreManager 验证：新 helper 使 dummy 内核以 0 退出并完成清理，宿主始终无 console；旧 helper 回退为强杀。真实 Meow 的网卡 DNS 恢复仍需随包验收。

## 发布约束

GUI 使用随包的 `nyanpasu-service.exe` 作为 helper。只更新 GUI 而沿用旧 service release 时，helper 不识别入口，退回强杀。含 helper 入口的 service 已随 runtime `v2.0.1` 发布，本 PR 的 runtime gitlink 指向该 tag，`prepare:check` 会下载这一版本。

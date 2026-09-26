# T9 IPC、前端与 legacy 入口换线

## 范围与假设

T8 专项验证通过后开始。IPC/Tauri 只做 DTO 转换、事件转发与错误映射；状态和修复入口放在 NyanpasuClient。legacy 输入保留字段转换，但删除第二套提交/补偿/效果编排。多行为域请求在任何写入前拒绝，产品不新增复合原子事务。

## 实施与验收

1. mutation wire 明确 committed，携带本次 source 提交身份/版本、runtime 结果和 notifications_pending；后台完成通过独立状态读取/事件展示。保存编辑器也返回诚实回执。验证：pending GUI 不被描述为全部 applied，source 错误与异步效果失败区分。
2. 增加 facade/IPC convergence inspect、RetryNow 和版本约束 repair；事件按同一会话内递增序号接收。前端提供独立配置状态区域，展示等待、阻塞、恢复需求与近期操作；transport 未确认不声称“未保存”。验证：bindings 生成、typecheck、状态序号过滤。
3. legacy patch 先纯转换并判定行为域，单域调用 typed facade，多域和混合 Session 在写入前拒绝。移除 saga/legacy effect gate，迁移窗口保存、启动回灌与 UI 的复合调用；无法表达的 legacy 字段明确拒绝。验证：跨域失败无任何 source/file/runtime 写入，单域所有入口均经过 participant。
4. 执行 IPC/bridge/actor/UI 回归、bindings、前端检查；记录结果。随后进入全量 review loop，处理先前记录的旧集成 fixture/断言、架构和取消恢复边界。

## 验证记录

- bindings 导出测试通过；原先未导出的 typed patch 包含与 legacy DTO 重名的 enum，因此 IPC repair 复用已定义的 IVerge DTO，在 bridge 纯转换后进入版本约束 typed facade，避免新增同名 wire 类型。
- bridge 回归 14 项通过；Application/Clash 与 Application/Session 混合请求均在 source 版本变化和文件写入前拒绝。
- 前端 typecheck 全部通过，frontend 测试 27 项通过（含旧成功晚到不得清除新失败、transport reply 丢失与确定领域拒绝区分）。
- effects 47 项通过，新增 committed receipt + pending notifications 集成用例通过。

## UI 调用清单

- useSettings 整体 patch：语言 provider 仅写 language，其他使用均读配置；没有需要保留跨域原子性的调用。
- useSetting 标量写：core、host、外观、系统代理、guard、快捷键和 widget 属 Application；TUN/stack、控制通道、端口、字段过滤、web UI、断连策略属 Clash。所有入口由单域 converter 路由。
- mixed-port-config 原来连续写 overrides 与 verge_mixed_port，改为一次 Clash port strategy 更新；tun-stack-selector 的 stack 与 enable 两个动作仍属于 Clash，按显式动作顺序返回各自结果。
- profiles 创建/导入与自动激活仍为独立提交，回执分别保留其 operation ID/version；editor 保存返回 MutationOutcome，交给 MutationCache 处理降级。
- 窗口持久保存直接调用 Session actor 的 SaveMainWindow，保留其他窗口条目；resize 的旧内存恢复投影明确留待 T11，不能作为持久 source。
- 删除启动后全量 legacy 回灌与 legacy 写盘 finalizer/saga。confirmed ports 仅作为运行期兼容投影，不写回 source 文件。

# Runtime 配置堆分配优化计划

**日期：** 2026-10-18

**状态：** 实施中；严格先完成借用式 diff，再实施 effective 共享。

**工作区：** 继续 `memory-profiling` worktree、`perf/provider-cache-summary` 分支及 Draft PR #5831。runtime 从主仓库当前锁定的 `9aaabb1cb97875860aa19af3c0947bbcd95b33aa` 切出 `perf/config-heap-sharing`，不混入上游其他改动。继续使用 worktree 自有的 `backend/target`；不创建跨 worktree 的共享可写 target。

依据 [堆分配调查](../review/2026-10-18-heap-allocation-profile.md)，本次依次处理以下两项；provider 摘要优化已经完成，其 [计划与结果](2026-10-18-provider-cache-summary.md) 保留原测量基线。

## 1. 借用式根 Mapping diff

`config/diff.rs::diff` 当前将两个借用的根 `Mapping` 克隆为临时 `Value::Mapping` 后才比较。改为直接借用根 mapping，复用同一递归比较逻辑，仅为输出的变更路径和 `DiffEntry.new` 分配。保持路径排序、增删语义、数组整体替换、空 mapping 与类型替换的行为。

验收：

- 覆盖相等但独立分配的树、大数组、嵌套增删、标量/mapping 替换、空 mapping 和确定的路径顺序；既有 Mihomo 分类测试通过。
- 使用包含实际 diff 源码的计数程序，将改动前后的函数放在同一程序中对照。原调查的 62,020 字节配置和 843,153 字节合成配置，Noop 临时分配分别为 936,102 和 7,600,876 字节；目标为 0，不把整个 apply 流程的其他准备成本算作 diff 成本。本次直接使用 provider 实验冻结的 62,021 字节 portable 产物，另外复用原合成配置。
- 测试和格式/Clippy 通过后，单独提交该完整阶段，再开始下一项。

## 2. effective 与发布快照共享

将 `EpochPlan.effective_document` 改为 `Arc<Mapping>`；仅在配置准备、控制通道和原生路径改写全部完成后，从拥有的 mapping 构造一次 Arc。冷启动、普通 apply 和 graceful full-plan 构建均使用这一规则。发布 `EffectiveConfigSnapshot` 与 `effective_config()` 查询只克隆 Arc，继续在原控制锁下绑定实例身份和 revision。

source mapping 保留独立所有权；不让 source 指向 effective，不改写已发布的 mapping，不共享 mutable mapping，不使用 `Arc::make_mut` 延迟掩盖复制。graceful bootstrap、配置分类、序列化顺序、语义哈希、落盘产物、提交/恢复时序和 revision fencing 均沿用原语义，公共快照类型不变。

验收：

- 查询之间、查询与发布快照使用 `Arc::ptr_eq` 验证共享；Noop 保持原文档分配。
- 持有旧快照后进行 patch、reload/restart、hard/graceful switch，旧文档保持不变，新 revision 使用对应的新文档；失败回滚恢复旧计划的内容与共享关系。
- 运行 manager 单元测试和相关集成测试，覆盖配置 apply、control channel、graceful、恢复、原生路径和 reconcile；检查父仓库消费者可编译。
- 使用冻结 portable 输入做改造前后 DHAT 对照，检查 effective 发布栈深拷贝消失、三份 Mapping 减为两份；预计回收本样本一棵树约 468,051 字节（0.446 MiB），实际结果单独记录。临时 diff 收益与常驻收益分别报告。

## 交付与复核

两个阶段按顺序形成独立、可构建的 runtime 提交，推送可获取的子模块提交，再更新主 PR 的 gitlink 和计划验证记录。PR 标题与描述按最终 provider + 配置堆优化范围重写，并关联 runtime 变更供审阅。原始配置、节点数据、DHAT 和诊断计数程序保留在 gitignored target，不提交私有输入。

最终复核：每行变更对应上述目标；没有全局服务状态、Tauri 依赖或兼容层；runtime 自身 rustfmt/Clippy 与父仓库相关检查通过；明确区分单次 Noop 临时分配、Rust 存活堆和系统 RSS。

## 实施记录

### 第一阶段：借用式 diff

根 mapping 直接借用，根与递归值均先比较相等性；只有不相等的 mapping 才收集、排序其借用的字符串键。叶子替换继续克隆输出值，空 mapping 与非字符串键的原有退化行为保留。没有为了借用而引入临时 `Value` 包装或新的缓存。

同一计数程序包含锁定基线的旧 diff 与改造后的实际源码；先构造两个独立但内容相等的 mapping，再单独计数 diff 调用。另对 169 组空值、类型与嵌套组合逐一核对新旧函数输出，全部一致。额外以根标量字段增量检查输出等价性和非 Noop 分配：

| 输入               | YAML 字节 | 原 Noop 分配 | 借用式 Noop 分配 | 原单字段变更分配 | 借用式单字段变更分配 |
| ------------------ | --------: | -----------: | ---------------: | ---------------: | -------------------: |
| 冻结 portable 配置 |    62,021 |      936,104 |                0 |          937,650 |                1,523 |
| 原合成大配置       |   843,153 |    7,600,876 |                0 |        7,601,758 |                  859 |

portable 产物比原调查输入多 1 字节，一棵树实测为 468,052 字节，因此旧 diff 的复制成本比原记录多 2 字节；对照始终让新旧函数读取同一输入。结果与输入哈希保存在 `backend/target/runtime-config-measurement/diff-allocation.json`，计数程序在同目录 `diff-probe/`，旧源码保存在 `diff-before.rs`。这些临时工具与私有输入不提交。

改动前配置相关单元测试 23 项通过；增加 7 项 diff 测试后配置相关 30 项通过。扩大运行全部 manager 单元测试，122 项通过、1 项按原标记忽略，另有 2 项原生路径测试在 Windows 失败：预期硬编码 `/data/ui` 与 `/data/providers/local.yaml`，实际为 Windows 分隔符。切回原始 diff 源码、强制重新编译后相同两项仍失败（原始共 118 项，选中 2 项、过滤 116 项），证据为 `baseline-windows-path-tests.log`；本次没有修改这些无关路径逻辑或隐藏失败。

第一阶段 runtime Clippy（全部 targets/features）与该包 rustfmt 检查通过，提交为 `0ee1054`，已推送子模块分支。Clippy 仍有既有告警，本次 diff 文件没有新增告警。包含此阶段、尚未共享 effective 的 profiling 基线已构建成功（10 分 44 秒），正在从冻结输入采样，以便第二阶段只测共享改动的常驻收益。

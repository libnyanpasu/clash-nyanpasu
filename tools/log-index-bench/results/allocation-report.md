# Log index allocation benchmark — 2026-09-11

Repository revision: `705650033c13ae345b058c1a29a81b80017caf73`.
Reference indexer SHA-256: `2ca607b0a09f6e9c3bf8e2eb1923aea387516e200d58831913a45781a1d01a9c`.

结论：Bump 可以修复释放问题，不需要先迁移成标准库。移除 Bump 的成本取决于具体布局；同结构 owned tree 与 dense Vec/TargetId 必须分开比较。

## 方法与限制

- Windows x86_64 / Intel Core i9-14900KF；主线程固定在逻辑 CPU 0。Rust 1.100.0-nightly (67eda617e 2026-09-10)，release，codegen-units=1，System allocator。
- 每组 1 轮预热、9 轮采样；逐轮轮换候选执行顺序。表内耗时为中位数；原始 JSON 同时保留 min/max/p90。
- 三组确定性合成负载：10 万条 / 32 个 64-byte target；10 万条 / 5 万个 128-byte target；100 万条 / 128 个 64-byte target。target 均匀分布。
- 测试输入已提供借用字符串，计时包含候选内部字符串复制、分配和索引维护；不包含 JSON/时间解析、磁盘、IPC、前端。去重方案在当前 String 反序列化路径下的总收益尚未测量。
- 所有方案保存 line/level/timestamp/target/start_pos/end_pos，输出同一拥有 String 的 LogEntry DTO。所有 query 的全部字段校验和与参考实现一致。
- 同结构组保留原有 BTreeMap 行索引、时间索引、级别/target postings 以及查询算法。Bump baseline 只增加析构；未改建索引和查询代码。Fx 组统一使用原有 rustc-hash。default hash 组是标准库集合和哈希器。
- 只读组对 target 去重。Bump 组使用 alloc_str、arena 内的只读条目及 self_cell，postings Vec 在 arena 外由普通集合拥有。标准库 Arc 组仅每个唯一 target 创建 Arc 分配。
- Vec + TargetId 组额外改变行索引布局：连续行号直接索引 Vec；每条记录只存 TargetId，字符串表按唯一 target 持有 Arc<str>。这是独立优化组，不能将全部收益归于分配器。Vec 不预先预留容量；保留自然扩容成本。
- 原有时间查询缺陷不在本探针修复范围。数据使用唯一、递增时间戳和行号，避免用不正确的查询语义比较容器。
- 查询维持原来的临时 Vec/BTreeSet 生成方式，包括 latest 先生成全部行号再 offset 的行为。本文不代表优化后的查询复杂度。
- 分配统计通过独立 alloc-stats 构建运行；耗时构建没有统计原子操作。内存为分配器请求的存活字节，排除预建输入，不是 RSS、提交页或分配器内部开销。
- 所有候选完整创建/追加/查询/销毁后净残留为 0 字节；另各做 100 个小规模重复生命周期，逐次检查净残留为 0。没有故障注入或生产进程 RSS 测试。
- Bump+self_cell、std owned/Arc/Vec 候选通过编译期 Send + static 检查；baseline 原始裸指针组不宣称 Send。

## 数据

Build/append/drop 单位 ms；查询单位 μs；live 是初次构建完成时的 MiB。Alloc 不含另列的 realloc。

### 100,000 rows / 32 targets

| Variant                                    | Build ms | Append 1000 ms | Drop ms | Live MiB |   Alloc | Realloc |
| ------------------------------------------ | -------: | -------------: | ------: | -------: | ------: | ------: |
| Bump + String, explicit Drop               |   17.411 |          0.178 |   4.101 |   22.810 | 233,390 |     415 |
| Std tree + String, Fx                      |   18.302 |          0.181 |   4.550 |   24.603 | 233,376 |     415 |
| Std tree + String, default hash            |   19.893 |          0.188 |   4.487 |   24.603 | 233,376 |     415 |
| Bump interned str + self_cell, Fx          |   12.582 |          0.143 |   2.362 |   16.705 |  33,391 |     415 |
| Std tree + interned Arc<str>, Fx           |   13.563 |          0.109 |   2.796 |   17.100 |  33,408 |     415 |
| Std tree + interned Arc<str>, default hash |   16.369 |          0.137 |   2.727 |   17.100 |  33,408 |     415 |
| Std Vec + TargetId, default hash           |    8.672 |          0.071 |   1.166 |   11.440 |  16,744 |     433 |

| Variant                                    | Latest 100 | WARN+ERROR |  Target | Time 1000 | Combined |
| ------------------------------------------ | ---------: | ---------: | ------: | --------: | -------: |
| Bump + String, explicit Drop               |     13.777 |    555.800 | 115.282 |     3.807 | 3056.000 |
| Std tree + String, Fx                      |     13.730 |    556.725 | 116.629 |     3.810 | 3036.800 |
| Std tree + String, default hash            |     13.738 |    549.225 | 114.553 |     3.898 | 3055.400 |
| Bump interned str + self_cell, Fx          |     14.477 |    561.400 | 114.200 |     4.065 | 3043.700 |
| Std tree + interned Arc<str>, Fx           |     14.156 |    559.875 | 117.206 |     4.223 | 3048.200 |
| Std tree + interned Arc<str>, default hash |     14.103 |    553.675 | 118.000 |     4.248 | 3032.900 |
| Std Vec + TargetId, default hash           |     13.172 |    550.625 | 113.200 |     3.235 | 3081.300 |

### 100,000 rows / 50,000 targets

| Variant                                    | Build ms | Append 1000 ms | Drop ms | Live MiB |   Alloc | Realloc |
| ------------------------------------------ | -------: | -------------: | ------: | -------: | ------: | ------: |
| Bump + String, explicit Drop               |   35.319 |          0.317 |   7.808 |   37.602 | 283,368 |  50,063 |
| Std tree + String, Fx                      |   37.516 |          0.402 |   7.635 |   40.393 | 283,354 |  50,063 |
| Std tree + String, default hash            |   39.205 |          0.423 |   7.186 |   40.393 | 283,354 |  50,063 |
| Bump interned str + self_cell, Fx          |   29.014 |          0.228 |   3.260 |   27.791 |  83,370 |  50,063 |
| Std tree + interned Arc<str>, Fx           |   30.608 |          0.270 |   4.821 |   27.050 | 133,354 |  50,063 |
| Std tree + interned Arc<str>, default hash |   35.884 |          0.297 |   4.579 |   27.050 | 133,354 |  50,063 |
| Std Vec + TargetId, default hash           |   31.087 |          0.206 |   3.525 |   22.889 | 116,690 |  50,092 |

| Variant                                    | Latest 100 | WARN+ERROR | Target | Time 1000 | Combined |
| ------------------------------------------ | ---------: | ---------: | -----: | --------: | -------: |
| Bump + String, explicit Drop               |     15.302 |    550.325 |  0.256 |     5.247 | 2876.100 |
| Std tree + String, Fx                      |     14.003 |    548.650 |  0.254 |     3.980 | 2883.400 |
| Std tree + String, default hash            |     14.355 |    550.000 |  0.270 |     3.985 | 2860.700 |
| Bump interned str + self_cell, Fx          |     14.245 |    545.750 |  0.252 |     4.065 | 2853.100 |
| Std tree + interned Arc<str>, Fx           |     14.070 |    553.425 |  0.259 |     3.937 | 2856.600 |
| Std tree + interned Arc<str>, default hash |     13.996 |    555.900 |  0.278 |     3.957 | 2900.000 |
| Std Vec + TargetId, default hash           |     12.973 |    550.300 |  0.262 |     3.137 | 2831.000 |

### 1,000,000 rows / 128 targets

| Variant                                    | Build ms | Append 1000 ms | Drop ms | Live MiB |     Alloc | Realloc |
| ------------------------------------------ | -------: | -------------: | ------: | -------: | --------: | ------: |
| Bump + String, explicit Drop               |  177.099 |          0.189 |  39.445 |  207.819 | 2,333,487 |   1,615 |
| Std tree + String, Fx                      |  187.700 |          0.216 |  38.616 |  241.733 | 2,333,470 |   1,615 |
| Std tree + String, default hash            |  203.891 |          0.207 |  39.326 |  241.733 | 2,333,470 |   1,615 |
| Bump interned str + self_cell, Fx          |  132.238 |          0.134 |  19.617 |  146.778 |   333,488 |   1,615 |
| Std tree + interned Arc<str>, Fx           |  142.008 |          0.135 |  22.144 |  166.710 |   333,598 |   1,615 |
| Std tree + interned Arc<str>, default hash |  169.475 |          0.154 |  21.444 |  166.710 |   333,598 |   1,615 |
| Std Vec + TargetId, default hash           |   91.082 |          0.083 |   7.122 |   98.096 |   166,936 |   1,638 |

| Variant                                    | Latest 100 | WARN+ERROR |  Target | Time 1000 |  Combined |
| ------------------------------------------ | ---------: | ---------: | ------: | --------: | --------: |
| Bump + String, explicit Drop               |   1080.400 |   6328.000 | 289.600 |     4.542 | 30574.400 |
| Std tree + String, Fx                      |   1094.000 |   6176.600 | 291.771 |     4.543 | 30753.000 |
| Std tree + String, default hash            |   1080.200 |   6131.500 | 289.557 |     4.529 | 30708.800 |
| Bump interned str + self_cell, Fx          |   1080.250 |   6089.600 | 288.557 |     4.644 | 31190.200 |
| Std tree + interned Arc<str>, Fx           |   1093.900 |   6211.600 | 291.886 |     4.929 | 31297.900 |
| Std tree + interned Arc<str>, default hash |   1084.850 |   6226.600 | 288.543 |     4.860 | 31183.900 |
| Std Vec + TargetId, default hash           |   1074.350 |   6224.700 | 287.650 |     3.420 | 31196.900 |

## 可选实现路径

1. 保留 Bump，使用 alloc_str + intern，只有无需析构的借用/数值条目进入 arena；可增长的普通 Vec 由外层容器拥有；用 self_cell 封装移动和析构顺序。
2. 最小释放修复可由外层容器持有 bumpalo::boxed::Box<LogEntry> 或 Box<Vec<_>>，让 RAII 执行析构。不要再把拥有 Box 的包装器用 Bump::alloc 隐藏而跳过析构。该 RAII Box 变体本轮没有单独计时，baseline 使用显式 drop_in_place 作为控制组。
3. readonly 不等于无释放责任。普通 Box<str>/Arc<str>、长 SmolStr/CompactString 仍需 Drop。SmolStr 的 inline 优化只覆盖最多 23 bytes 等特例，不能当任意 target 的 arena 泄漏修复。
4. Bump 内部增长数组可用 bumpalo::collections::Vec，但不能假定它拥有与普通 Vec 相同的 Send/扩容回收性质。当前验证路径将 postings 保留为普通 Vec。

## 重现

在当前目录运行；现有生成源码已自包含，无需运行 generator：

```powershell
cargo run --offline --release --bin allocation_bench -- timing 1> allocation-timing.jsonl 2> allocation-timing-progress.log
cargo run --offline --release --features alloc-stats --bin allocation_bench -- memory 1> allocation-memory.jsonl 2> allocation-memory-progress.log
```

Cargo.lock 固定依赖。reference-indexer.rs 保存原代码，generate_allocation_bench.py 保存各候选变换方式。无生产仓库文件修改。

来源：[Bump 的析构行为](https://docs.rs/bumpalo/latest/bumpalo/struct.Bump.html#no-drops)、[arena Box](https://docs.rs/bumpalo/latest/bumpalo/boxed/struct.Box.html)、[self_cell](https://docs.rs/self_cell/1.3.0/self_cell/)、[SmolStr](https://docs.rs/smol_str/0.3.6/smol_str/struct.SmolStr.html)。

# Jobs P0 验证工程

用于验证 [P0 契约](../../design/jobs-p0-contract.md)依赖的库行为与候选 DTO，不是生产 Jobs 实现。独立 Cargo workspace，不加入 backend workspace，不访问应用数据目录；redb 测试使用临时目录。

## 运行

从仓库根目录执行：

```sh
cargo test --manifest-path docs/probes/jobs-p0/Cargo.toml --locked
cargo clippy --manifest-path docs/probes/jobs-p0/Cargo.toml --locked --all-targets -- -D warnings
cargo fmt --manifest-path docs/probes/jobs-p0/Cargo.toml --check
```

首次运行需要下载 Cargo.lock 中的依赖。产物仅写入此工程自己的 `target/`。遵循仓库 Rust toolchain；资源受限时可设置 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`。

## 验证范围

| 文件                 | 测试数 | 观察                                                                                                                                 |
| -------------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `tests/schedule.rs`  | 6      | 五/六字段与 exclusive 起点；非法表达式/年字段/时区；无有效日期；纽约 DST gap/overlap；interval 首次延迟及 Skip 仍交付一次逾期 tick。 |
| `tests/lifecycle.rs` | 2      | abort 不能停止已开始的 blocking 工作；wait 超时不取消 owner，其他及晚到 waiter 仍收到结果。                                          |
| `tests/storage.rs`   | 2      | 独占数据库文件；事务 abort 不留下半条记录/索引；终态与日志事务一致；游标使用接纳序号。                                               |
| `tests/dto.rs`       | 2      | 候选有限 DTO 可由 Specta 导出；大整数使用字符串；业务成功、journal 降级和核心应用未知可独立表示。                                    |
| `tests/logging.rs`   | 1      | 全局过滤提前丢事件；独立 layer 过滤允许 journal 保留事件。                                                                           |

固定 croner 4.0.0、chrono 0.4.45、chrono-tz 0.10.4；redb 4.2.0、Tokio 1.53.1、Specta 2.0.0-rc.25 及 serde/typescript 适配 0.0.12 与核对时的应用依赖保持一致。完整版本以 Cargo.toml/Cargo.lock 为准。

2026-09-07：13 项测试通过。测试以显式确认、虚拟时间及临时数据库运行；blocking 探针的超时是避免测试挂住的保护，不用于猜测任务是否完成。

这里没有 JobsActor、真实 Profiles workflow 或应用 IPC。事务 abort 不模拟 ENOSPC/断电，虚拟时间不模拟真实 OS suspend，候选 DTO 的导出不替代未来应用绑定生成。P1–P7 仍须按实施计划完成状态机、故障注入和集成验收。

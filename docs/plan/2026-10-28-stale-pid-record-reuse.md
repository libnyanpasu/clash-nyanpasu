# 残留 pid 记录遇到 PID 复用导致启动崩溃（#6033）修复计划

**日期：** 2026-10-28

**状态：** utils#209 与 runtime#462（`90cf8c6`）已合入，runtime `v2.0.2`（`e73b9f5`）已发布；app 在 `fix/require-service-2.0.2` 上 pin 到 `v2.0.2` 并把服务最低版本提到 `2.0.2`。实施中对计划的修正见文末“实施记录”。

**基线：** app `main@80dc5656f`（v2.0.0）；runtime pin `4eccc55`（与 `origin/main@6598188` 在相关文件上无差异）；utils pin `b1a154f`（与 `origin/main@312a91d` 在 `src/process/` 上无差异）。

下文路径前缀：`utils/` = `backend/nyanpasu-runtime/crates/nyanpasu-utils/`，`manager/` = `backend/nyanpasu-runtime/crates/nyanpasu-core-manager/`，`service/` = `backend/nyanpasu-runtime/crates/nyanpasu-service-runtime/`。

## 1. 已确认的根因

macOS 用户升级到 2.0.0 后启动即 panic，日志唯一原因是 `cannot prove ownership of live pid 1332`。用户删除 `~/Library/Application Support/clash-nyanpasu/config/runtime/control/core-*.pid` 后恢复正常，根因确认。

调用链：

1. `backend/tauri/src/lib.rs:343` setup → `backend/tauri/src/core/actor_v2/local_host.rs:44` 构造 `CoreManager`；
2. `manager/src/manager/mod.rs:354` `sweep_orphans(..).await?`；
3. `manager/src/manager/quarantine.rs:148` 对残留 epoch 调 `reap_epoch_pid_file(..).await?`；
4. `utils/src/process/pid_file.rs:378-387` `reap_record`：PID 存活但 `(executable, start_token)` 与记录不符，返回 `PermissionDenied`。

记录只在回收成功后删除，因此每次启动都会以同样方式崩溃。service 守护进程也构造 `CoreManager`（`service/src/server/manager_bridge.rs:202`），失败时同样无法提供服务。

缺陷有两层：

- **语义错误。** 同一 PID 上启动时间不同，说明记录的进程已经退出、PID 被复用。`reap_record` 却把它当成“无法证明归属”。同文件的 `recorded_process_is_alive`（`pid_file.rs:220-228`，“A missing/reused PID is dead”）把同样的情形判为已退出，两处不一致。现有测试 `utils/tests/process_pid_file.rs:268` 和 `manager/README.md:455` 把错误行为写成了契约。
- **macOS 身份读取有损。** macOS 身份经 sysinfo 0.39.6 读取（`pid_file.rs:629-648`）。读另一 uid 的进程时 `PROC_PIDTBSDINFO` 被拒，sysinfo 退回 `new_empty`，启动时间记为 `0`（`sysinfo-0.39.6/src/unix/apple/macos/process.rs:57-74,371-414`）。启动时间读成 `0` 时无从判断失败原因，所以不能当作“另一个进程”的证据。低位 PID 多为 root 守护进程，#6033 很可能正是这种情况。

已排除：nightly 与 2.0.0 之间身份算法变化（`process_identity`/`reap_record` 自 utils `c6148c5`，2026-07-19 起未改；sysinfo 自 2026-08-15 起恒为 0.39.6）。

## 2. 成功判据

1. 残留记录指向一个启动时间可靠不同的存活进程时，`reap_epoch_pid_file` 返回 `AlreadyExited` 并删除记录，`CoreManager` 正常构造。
2. 该外部进程及其子进程不收到任何信号。
3. 上一条在 macOS 上对**另一 uid**（含 root）的进程同样成立。
4. 读不到身份（真正无法检查）仍是错误，绝不被当作“已退出”或“可杀”。
5. 身份完全匹配的孤儿内核照旧被杀并确认死亡。
6. 上述测试在 utils/runtime 的 CI 上**真的执行**（目前只编译不执行，见 §4.4）。

## 3. 方案取舍（联合讨论结论）

| 议题                                  | 结论                                                                                                     | 依据                                                                                                                                                                                                                                                  |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 任何身份不符都算“已退出”？            | **否**。只有“启动时间可靠不同”才算已退出；启动时间相同而可执行文件名不同，仍视为不确定。                 | Linux 上同一进程的 `/proc/pid/exe` 会因 `execve` 或二进制被删而变化；macOS sysinfo 的 exe 来源会在 `KERN_PROCARGS2` 与 `proc_pidpath` 之间切换。                                                                                                      |
| 把 sysinfo 的 `0` 当作“外部 uid”？    | **否**。                                                                                                 | `0` 丢掉了失败原因（MAC 策略拒绝、进程正在退出都会产生它）；native-store 的 `active` 记录在 GUI 与 service 之间共享（`service/src/server/mod.rs:114-138`，`manager/src/native_store/fs.rs:118-133`），按 GUI 视角推断 uid 会误放行 service 的活内核。 |
| macOS 怎么拿到可靠的启动时间          | 用 `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID)` 原生读取 `p_starttime.tv_sec`。保留秒精度和记录格式 v2。 | 该接口不做 uid 过滤，与 `pbi_start_tvsec` 同源。`libc` 0.2.191 只有 `KERN_PROC_PID` 常量，没有 `kinfo_proc` 绑定，需要本地声明一个前缀结构（替代方案的排除见 §3.1）。                                                                                 |
| Linux 二进制升级后 `mihomo (deleted)` | **纳入**：启动时间相等时，观察到的文件名比记录多一个结尾的 ` (deleted)` 也算匹配。                       | 否则升级后仍在运行的孤儿内核会被判为不确定，启动失败。只放宽比较，不全局剥后缀。                                                                                                                                                                      |
| 去掉子进程的 `unowned_is_dead`        | **不做**，列为后续。                                                                                     | 修复点在子进程捕获之前返回，与它无交集。                                                                                                                                                                                                              |
| 启动时把失败 epoch 隔离、不让构造失败 | **叠在本修复之后**，单独讨论、单独 PR（见 §5）。                                                         | 最终目标是启动时不再出现 setup panic；本修复先消除 PID 复用这一来源，其余来源在检测修复合入后再定方案。                                                                                                                                               |
| 旧 macOS 记录里的 `start-token=0`     | 不特殊处理。                                                                                             | 原生读数永不为 `0`，与之必然不同，按已退出处理、不杀进程。最坏结果是漏杀一个孤儿内核，导致端口冲突，不会崩溃。                                                                                                                                        |

### 3.1 macOS 绑定的替代方案（已逐一排除）

需求：以普通用户身份读取**任意 uid**（含 root）进程的启动时间，失败原因可区分。依据 XNU `xnu-11417.140.69` 源码核对：

| 候选                                                           | `libc` 绑定 | 跨 uid                                                                                                               | 启动时间                      | 排除理由                                                                                        |
| -------------------------------------------------------------- | ----------- | -------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ----------------------------------------------------------------------------------------------- |
| `proc_pid_rusage`（`ri_proc_start_abstime`）                   | 有          | 否                                                                                                                   | 有                            | `bsd/kern/proc_info.c:3834` 以 `CHECK_SAME_USER` 检查，对其他 uid 返回 `EPERM`，与现状同病      |
| `proc_pidinfo(PROC_PIDT_SHORTBSDINFO)`                         | 有          | 是（`proc_info.c:2197-2203` 免检）                                                                                   | 无                            | `struct proc_bsdshortinfo`（`bsd/sys/proc_info.h:85-99`）只有 pid、ppid、状态、comm、uid 等字段 |
| `proc_pidinfo(PROC_PIDUNIQIDENTIFIERINFO)`                     | 无          | 是                                                                                                                   | 唯一 id                       | 结构体不在公开头文件 `proc_info.h` 中，属于 Apple 私有 API，比本地绑定公开 API 更糟             |
| `proc_pidinfo(PROC_PIDTBSDINFO)` 并读 errno                    | 有          | 否                                                                                                                   | 有                            | 只能拿到 `EPERM`，无法区分“其他 uid”与 MAC 策略拒绝                                             |
| 子进程调用 `ps -o lstart= -p <pid>`                            | 不需要      | 是                                                                                                                   | 有                            | 每次查询都起子进程，输出格式随 locale 变化；`ps` 内部用的也是 `KERN_PROC_PID`                   |
| 第三方 crate（`libproc`、`darwin-libproc`、`mach2`、`sysctl`） | —           | —                                                                                                                    | —                             | 前两者封装的是同样受 uid 检查的 `proc_pidinfo`；后两者不提供 `kinfo_proc`                       |
| **`sysctl(KERN_PROC_PID)`**                                    | 仅常量      | 是：过滤函数只比 pid（`bsd/kern/kern_sysctl.c:719-725`），遍历 `PROC_ALLPROCLIST \| PROC_ZOMBPROCLIST`（`:917-918`） | `p_starttime`（`:1192-1193`） | **采用**                                                                                        |

结论：需要本地绑定的只是一个**公开** API：`struct kinfo_proc` 在 SDK 公开头文件 `<sys/sysctl.h>` 中，Apple 技术问答 QA1361 也直接使用它。`libc` crate 缺这个绑定。`p_starttime` 是 `kp_proc.p_un` union 的第一个成员，位于结构体偏移 0，所以只需声明到 `p_pid`、`p_stat` 为止的前缀；缓冲区按完整的 `sizeof(struct kinfo_proc)` 分配，并校验返回长度。

绑定处必须附说明注释，格式与 `// SAFETY:` 并列，草稿如下（实施时按实际偏移补全）：

```rust
// FFI-BINDING: `libc` does not expose Darwin's `struct kinfo_proc`
// (<sys/sysctl.h>, a public SDK header), so this module declares the prefix
// it reads. `sysctl(KERN_PROC_PID)` is the only public interface that returns
// a process start time for a process owned by another uid: `proc_pidinfo`'s
// BSD info and `proc_pid_rusage` reject other uids (EPERM), short BSD info
// has no start time, and the unique-identifier flavor is Apple-private.
// Layout: `p_starttime` is the first member of `kp_proc.p_un`, offset 0; the
// remaining fields mirror `struct extern_proc` up to `p_stat` for arm64 and
// x86_64. The buffer is sized to the full `kinfo_proc` and the returned
// length is checked before any field is read. Tests compare the decoded
// start time and pid with `proc_pidinfo(PROC_PIDTBSDINFO)` for a same-uid
// process on both architectures.
// Removal: replace with `libc::kinfo_proc` once the `libc` crate binds it.
```

每个读取该缓冲区的 `unsafe` 块仍需单独写 `// SAFETY:`，说明长度已校验、对齐满足。

## 4. 实施

### 4.1 utils PR（`LibNyanpasu/nyanpasu-utils`）

**U1 区分三种身份比较结果。** 改造 `record_matches_identity`（`pid_file.rs:968-971`）的调用点，使其区分三种结果：

- 匹配：启动时间相等，且文件名相等或命中 U3；
- 已复用：启动时间不同；
- 不确定：启动时间相等而文件名不同。

改动点：

- `reap_record` 首次检查（`:382-387`）：已复用返回 `AlreadyExited`，且必须在 `capture_descendants`（`:389`）**之前**返回，避免波及外部进程的子进程；不确定仍返回错误。
- `reap_record_with_kill` 第二次检查（`:438-446`）：同样把已复用视为 `AlreadyExited`。
- `recorded_process_is_alive`（`:223-227`）：已复用返回 `false`，不确定返回错误。这对 `native_store/fs.rs:129-132` 的含义是：不确定时报 `Recovery`，不再静默删除 `active`。
- 不新增选项和类型层次；保留 Windows 同句柄、Linux pidfd、Unix 杀前复验的最终校验（`:829-965`）。

**U2 macOS 原生读取启动时间。** 把 `#[cfg(all(not(windows), not(target_os = "linux")))] process_identity`（`:629-649`）中 macOS 的启动时间来源换成 `KERN_PROC_PID`：

- 校验返回长度与 pid；
- 进程不存在（返回长度为 0）时返回 `Ok(None)`；
- 僵尸进程按已退出处理；
- syscall 失败时返回错误，绝不伪造成 `0`；
- 可执行文件名仍取自 sysinfo；在读文件名前后各读一次启动时间并比对，防止拼出混合身份；
- 其他 BSD 平台保持现状。

绑定声明附 §3.1 的 `FFI-BINDING` 说明，每个 `unsafe` 块写 `// SAFETY:`。

**U3 Linux `(deleted)` 后缀。** 仅在 Linux、仅在启动时间相等时，接受“观察到的文件名 = 记录的文件名 + ` (deleted)`”为匹配。改在比较处（`:968-981`），不改 `exe_names_equal` 对 spawn 时 `expected_exe` 的校验（`:148-152`）。

**U4 测试**（每条注明删掉哪行生产代码会让它变红）：

| 测试                                                                                                  | 断言                                                                                                                                       | 防护的生产行                               | 平台                           |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------ | ------------------------------ |
| 反转 `orphan_reap_rejects_unproven_start_identity_without_killing`（`tests/process_pid_file.rs:268`） | 写入一个明确不同的有效启动时间后 reap，得到 `AlreadyExited`，记录被删，子进程存活                                                          | U1 首次检查                                | 三平台                         |
| 外部进程未被杀                                                                                        | 用 `echo-stdin` helper，reap 后发 nonce，必须收到回显                                                                                      | U1 在捕获子进程前返回                      | 三平台                         |
| 外部进程树未被杀                                                                                      | 用现有孙进程 helper，根和子孙都存活                                                                                                        | 同上（删掉后会走子进程回收）               | 三平台                         |
| 第二次检查                                                                                            | 直接调私有 `reap_record_with_kill`，kill 闭包计数必须为 0，结果为 `AlreadyExited`                                                          | U1 第二次检查                              | 三平台                         |
| 文件名单独不同                                                                                        | 启动时间相同、文件名不同，返回错误，记录保留，进程存活；`recorded_process_is_alive` 返回错误                                               | U1 的不确定分支                            | 三平台                         |
| macOS 启动时间与 sysinfo 一致                                                                         | 对本进程，原生读数等于 `pbi_start_tvsec`                                                                                                   | U2 解码（arm64 与 x86_64 各跑一次）        | macOS                          |
| macOS 外部 uid                                                                                        | 用 `sudo` 起一个 root helper；构造启动时间不同的记录，reap 后 helper 仍回 nonce；与它匹配的副本记录，`recorded_process_is_alive` 为 `true` | U2（回退到 sysinfo 的 `0` 会让后半条变红） | macOS（CI runner 有免密 sudo） |
| Linux 二进制被删                                                                                      | 拷贝可执行文件运行，记录后 unlink，reap 得到 `Killed`；文件名无关的负例不被杀                                                              | U3                                         | Linux                          |

保留现有的匹配根进程、孙进程回收、路径逃逸、kill 错误暂定这几个测试；`pid_file.rs:1279-1293` 的子进程测试不动。

**U5 CI 与文档。**

- `utils/.github/workflows/ci.yml` 现在只跑 clippy，加上 `cargo test` 的三平台执行。
- 更新 `pid_file.rs:68-73`（`AlreadyExited` 包含 PID 复用）、`:220-245` 的文档，以及 utils README 中对应的契约描述。

### 4.2 runtime PR（`libnyanpasu/nyanpasu-runtime`）

- bump utils gitlink 到 U 合入后的提交。
- manager 回归测试：扩展 `manager/tests/manager_orchestration.rs:615` 的 sweep 测试，加入“残留记录指向启动时间不同的存活进程”。断言构造成功、残留物被清理、外部进程存活、下一个 epoch 高于已发现的最大值。
- `runtime/.github/workflows/ci.yml:50-62` 目前只测 jobs/serde/clash-api，需加入 `nyanpasu-core-manager` 的相关测试执行。
- `manager/README.md:454-468`：已证明的 PID 复用不再导致构造失败；真正无法验证的记录仍然导致构造失败（本次不改这一点）。
- 发布 patch 版本（暂定 v2.0.2）。`nyanpasu_service` 与 service-runtime 版本保持一致，并发布全平台 `nyanpasu-service` sidecar。

### 4.3 app PR

- runtime 发布后，bump `backend/nyanpasu-runtime` pin 到该 tag 并更新 lockfile。
- **锁步约束：** `scripts/src/prepare/binaries.ts:231-258` 按 `nyanpasu_service/Cargo.toml` 的版本下载 runtime release 资产，不从源码构建。必须先有 release 再推进 pin，否则打包仍会带旧守护进程。
- 不在 setup 里加删 pid 文件之类的兜底；不改 RPC。
- 随下一个 app patch 版本发布，在 #6033 回复修复版本号。

### 4.4 验证

- utils/runtime：对应 `cargo test`、clippy、rustfmt 在三平台 CI 上通过；本机只有 Windows，`cfg(target_os = "macos")` 的代码只能靠 CI 编译和执行。
- 打包冒烟（macOS）：手工放一个指向 root 进程 PID（如 `launchd` 以外的某个 root 守护进程）的残留 `core-*.pid`，启动 app 能正常拉起内核，且该 root 进程不受影响。

## 5. 后续（不在本次）

| 项目                                              | 说明                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **（叠加 PR，下一步讨论）启动时不再 setup panic** | 最终目标。本修复合入后再定方案。已知仍会让构造失败、反复崩溃的来源：降级后遇到不认识的记录版本、数据目录迁移导致 `runtime_config` 无法 canonicalize（`pid_file.rs:265-273,1080-1084`），以及目录、锁、枚举失败（`manager/src/manager/mod.rs:314-354`）。codex 提出的候选最小做法：只包住 `quarantine.rs:148` 的单条记录回收，失败的非零 epoch 进入现有 quarantine 并继续清理后续 epoch。待讨论的缺口：`recover_quarantine` 会重跑同一个回收，`NotFound` 也不会清除隔离（`quarantine.rs:43-48`），用户目前没有界面内的出路；目录、锁类失败在 app 侧仍会 panic。 |
| setup 失败时的提示                                | 崩溃弹窗只显示“Failed to setup app”，不带错误链，用户需要翻日志。可以让 panic 信息带上原因链。                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 子进程 `unowned_is_dead`                          | 子进程检查出错被当成已死，需要单独评估生命周期影响。                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| 更强的 macOS 身份                                 | 秒精度可能碰撞；改为微秒或绑定启动周期的身份需要升级记录格式。                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Linux `/proc/pid/exe` 缺失被当成进程不存在        | 主线程退出而其他线程仍在的情况，需另行审计。                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

## 6. 待决策

1. ~~启动时隔离是否并入本次？~~ 已定（2026-10-28）：叠在本修复之后，目标是启动不再 setup panic，检测修复合入后再讨论。
2. ~~macOS 本地绑定？~~ 已定（2026-10-28）：替代方案均不可行（§3.1），采用本地声明的 `kinfo_proc` 前缀，并附 `FFI-BINDING` 说明。
3. **版本号：** utils `0.1.1`，runtime/service `v2.0.2`，app 下一个 patch。

## 实施记录（2026-10-28）

**对计划的修正**

- **Linux 有同类缺陷，一并修复。** `/proc/<pid>/exe` 对其他用户的进程返回 EACCES，旧实现因此整体报错；PID 被 root 进程复用时，Linux 同样会崩溃循环。`ProcessIdentity.executable` 改为 `Option`：只要启动时间不同就判定为复用，不再需要读出可执行文件；启动时间相同而可执行文件读不出来，判定为不确定。spawn 写记录时仍要求读到可执行文件；子进程捕获只收录能读到可执行文件的进程（与原先“读不到就不归属”一致）。
- **macOS 可执行文件仍取自 sysinfo**；sysinfo 列不出该进程时，可执行文件记为 `None`，不再报错。
- **`KinfoProcPrefix` 增加编译期偏移断言**（`start_time`=0、`stat`=36、`pid`=40，对应 XNU `user64_extern_proc`）；同时以 `[workspace]` 独立小 crate 在 WSL 里对 aarch64/x86_64-apple-darwin 跑了 clippy（含 `undocumented_unsafe_blocks`），零警告。
- 新增 `reused_pid_owned_by_root_is_retired`（unix）：残留记录指向 PID 1，以非 root 身份运行时，修复前 Linux 和 macOS 都会报错。
- runtime 版本由其 publish workflow 统一 bump（`cargo set-version`，排除 utils）；本次只手工 bump utils。
- **CI 首次执行这些测试，暴露出两个原有问题，一并修复。** legacy pid 文件写入后未 flush（macOS 读到空文件）；Linux 上 spawn 后立即读 `/proc/<pid>/exe` 可能读到父进程。后者的原因：processkit 在没有可写 cgroup 时退回进程组后端，不加 `pre_exec`，std 走 glibc `posix_spawn`（`CLONE_VM|CLONE_VFORK`）；内核在 `exec_mmap()` 里先唤醒 vfork 父进程（`mm_exit_exec_release`）再换 mm。主线 `fs/exec.c` 顺序未变，属 vfork 语义；processkit 3.3.4 / `main@ffcecc4` 也未处理。修法是写记录前在有界等待内轮询到可执行文件名匹配，启动时间跨 exec 不变，匹配到的仍是同一进程。
- **app 把服务最低版本提到 `2.0.2`。** 服务模式下内核由守护进程的 `CoreManager` 管理，`manager_bridge.rs` 构造失败会让守护进程启动失败，同样会因残留记录反复失败；提高最低版本让旧守护进程走启动时升级路径。

**验证**

| 平台         | 结果                                                                                                                                                                       |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows      | utils 单元测试 9/9、集成测试 13/13 通过；rustfmt 通过；clippy 无新增警告。之后本机出现大面积 ICE/`STR_SENTINEL`（与 2026-10-20 页缓存损坏同特征），runtime 测试改在 WSL 跑 |
| Linux（WSL） | utils 单元测试 11/11、集成测试 14/14；以 `nobody` 身份运行单元测试 11/11（已确认 `nobody` 读不到 `/proc/1/exe`）；manager sweep 测试 2/2；clippy 在改动文件上零警告        |
| macOS        | 本地无法完整编译（`kill_tree` 构建脚本需要 SDK）；只验证了 darwin 模块的类型检查和偏移断言。行为测试依赖 CI                                                                |
| 变异验证     | 把 `reap_record` 的 Reused 分支改回报错后，两个复用相关集成测试变红                                                                                                        |
| CI           | utils#209（`df446a7`）与 runtime#462（`a7e6f79`）三平台全绿。Linux exec 竞态本机未能复现：`nobody` 身份经 `strace` 确认走 vfork 路径，但修复前后各循环 200 次均 0 失败     |

**已知限制**

- Windows：PID 被受保护进程（PPL）复用时，`OpenProcess` 可能被拒，仍会报错。未在本次处理。

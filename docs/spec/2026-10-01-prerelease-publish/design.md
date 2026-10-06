# Prerelease 发布设计：Publish CI 支持 beta/rc 并适配 beta 更新频道

日期：2026-10-01
代码基线：`main @ 80960e638`；工作分支 `ci/publish-prerelease`（已有未提交改动，见 §9）
状态：待评审

## 0. 目标与约束

### 0.1 目标

1. Publish 工作流能发布 `X.Y.Z-beta.N` / `X.Y.Z-rc.N`，发布物进入 App 的 beta 更新频道。
2. 在当前 main 上能直接发出 `2.0.0-beta.1`，之后能发 `2.0.0-beta.N`、`2.0.0-rc.N` 和 `2.0.0` 正式版。
3. nightly 在版本序上始终领先已发布的 beta 和正式版。
4. 各平台安装包的系统版本号（NSIS、deb、rpm）排序正确：beta < rc < 正式版 < nightly，系统包管理器不能把正式版当成降级。

### 0.2 不变的部分

- App 的频道判定：`bundle.rs::compiled_channel` 以 `NYANPASU_VERSION`（取自根 `package.json`）是否带 prerelease 决定 Stable/Beta，`nightly` feature 决定 Nightly。不改 Rust 代码。
- App 内更新比较：`bundle.rs::is_newer_release` 用 `cmp_precedence`（忽略构建元数据），Stable 频道拒绝 prerelease。不改。
- 更新清单：`scripts/updater.ts` 已生成 `update.json`（stable）与 `update-beta.json`（beta），`release-channel.ts::selectChannelRelease` 已按频道选 release。不改。
- 配置迁移：`migration/runner.rs:532` `introduced_in_reached` 只比较 (major, minor, patch)，`2.0.0-beta.1` 会执行挂在 `2.0.0` 的迁移。不改。

### 0.3 约束（已核实）

| 约束                                                                                                                                                                                                    | 来源 / 证据                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Tauri `version` 必须是合法 semver，且 deb、rpm、AppImage、NSIS、macOS 共用这一个值                                                                                                                      | `tauri-utils 2.10.0` `config.rs` `version_deserializer`；bundler 各后端都取 `settings.version_string()` |
| `DebConfig` 没有版本或 revision 覆盖字段                                                                                                                                                                | `tauri-utils 2.10.0` `config.rs:341`                                                                    |
| `RpmConfig` 有 `release`（默认 `"1"`）与 `epoch`                                                                                                                                                        | `tauri-utils 2.10.0` `config.rs:440-458`                                                                |
| RPM Version 不允许 `-`                                                                                                                                                                                  | #3850                                                                                                   |
| Tauri 更新签名绑定版本，清单生成时校验签名版本等于 tag                                                                                                                                                  | #5464；`updater-platforms.ts::collectUpdaterPlatforms`                                                  |
| NSIS：比较用 `nsis_tauri_utils::SemverCompare`（semver 的 `Ord`），prerelease 排序正确，但构建元数据也参与比较；`VIProductVersion` 丢掉 prerelease；`allowDowngrades` 默认 true，`/UPDATE` 模式跳过判断 | 子代理核验 tauri-cli-v2.12.0、nsis_tauri_utils-v0.5.3（见 §8）                                          |

deb/rpm 排序实测（WSL Ubuntu，`dpkg --compare-versions` 与 `rpm.vercmp`）：

| 写法                                    | deb                                 | rpm       |
| --------------------------------------- | ----------------------------------- | --------- |
| `2.0.0+beta.1` 对 `2.0.0`               | 大于 ❌                             | 大于 ❌   |
| `2.0.0-beta.1` 对 `2.0.0`               | 大于 ❌（`beta.1` 被当成 revision） | 不合法 ❌ |
| deb `2.0.0~beta.1` 对 `2.0.0`           | 小于 ✓                              | —         |
| deb `2.0.0~beta.2` 对 `2.0.0~rc.1`      | 小于 ✓                              | —         |
| rpm `2.0.0-0.beta.1` 对 `2.0.0-1`       | —                                   | 小于 ✓    |
| rpm `2.0.0-0.beta.2` 对 `2.0.0-0.rc.1`  | —                                   | 小于 ✓    |
| `2.0.0` 对 `2.0.1+alpha.abc`（nightly） | 小于 ✓                              | 小于 ✓    |

## 1. 版本模型

### 1.1 版本串一览（以发布 `2.0.0-beta.1` 为例）

| 位置                                                  | 值                                  | 写入者                                                     |
| ----------------------------------------------------- | ----------------------------------- | ---------------------------------------------------------- |
| 根 `package.json`、`frontend/*/package.json`          | `2.0.0-beta.1`                      | `publish.ts`                                               |
| `backend/tauri/tauri.conf.json` `version`             | `2.0.0-beta.1`                      | `publish.ts`                                               |
| `backend/tauri/overrides/nightly.conf.json` `version` | `2.0.1`                             | `publish.ts`                                               |
| git tag / GitHub release                              | `v2.0.0-beta.1`，标记为 Pre-release | `publish.yml`                                              |
| App 内 `NYANPASU_VERSION`                             | `2.0.0-beta.1`，频道 Beta           | `build.rs` 读 `package.json`                               |
| Windows NSIS：`VERSION` / `DisplayVersion`            | `2.0.0-beta.1`                      | bundler                                                    |
| Windows NSIS：`VIProductVersion`                      | `2.0.0.0`                           | bundler（只影响文件属性）                                  |
| macOS `CFBundleShortVersionString`                    | `2.0.0-beta.1`                      | bundler                                                    |
| Linux 打包时的 tauri `version`                        | `2.0.0`                             | `prepare-release.ts --linux`（§3）                         |
| rpm Version-Release                                   | `2.0.0-0.beta.1`                    | `prepare-release.ts --linux` 写 `bundle.linux.rpm.release` |
| deb control `Version`                                 | `2.0.0~beta.1`                      | 打包后改写（§3.3）                                         |
| AppImage 更新签名中的版本                             | `2.0.0-beta.1`                      | `sign-nightly-updater.ts` 用 `package.json` 版本重签       |

### 1.2 发布基准：以 package.json 为准，版本清单预置为 `2.0.0-beta.0`

修订（2026-10-02）：评审要求按下 `prerelease` + `beta` 就发出 `2.0.0-beta.1`。tag 基准做不到：从 `v1.6.1` 出发，`prerelease` 得到 `1.6.2-beta.1`。因此保留以 `package.json` 为基准，并用一个单独的提交把版本清单预置为 `2.0.0-beta.0`。

版本清单（该提交修改）：根 `package.json`、`frontend/{nyanpasu,interface,utils}/package.json`、`backend/tauri/tauri.conf.json`，统一为 `2.0.0-beta.0`。`backend/tauri/overrides/nightly.conf.json` 保持 `2.0.0`，nightly 在首次发布前仍是 `2.0.0-alpha+<hash>`，发布时由 `publish.ts` 提到 `2.0.1`。

`publish.ts` 计算 `semver.inc(package.json 版本, releaseType, preid, "1")`：

| package.json   | 操作                        | 结果           |
| -------------- | --------------------------- | -------------- |
| `2.0.0-beta.0` | `prerelease` + beta         | `2.0.0-beta.1` |
| `2.0.0-beta.1` | `prerelease` + beta         | `2.0.0-beta.2` |
| `2.0.0-beta.2` | `prerelease` + rc           | `2.0.0-rc.1`   |
| `2.0.0-rc.1`   | `major` / `minor` / `patch` | `2.0.0`        |
| `2.0.0`        | `patch`                     | `2.0.1`        |

防护：

- 结果必须大于当前版本，否则失败。例如在 `2.0.0-rc.1` 上选 `prerelease` + beta 会得到倒退的 `2.0.0-beta.1`。
- 计算出的 tag 已存在则失败（例如草稿 release 被删但 tag 残留）。
- `publish.ts` 同时写 `frontend/utils/package.json`；原列表中的 `frontend/ui` 已不是一个包。

预置期间的影响：本地非 nightly 构建会按 `2.0.0-beta.0` 编译为 Beta 频道；nightly 构建由 `prepare-nightly.ts` 改写版本，不受影响。

被否决的方案：tag 基准（见上）；自由输入的 `version` 参数（需要额外校验，且容易输错）。

### 1.3 nightly 基准

`nightly.conf.json` 的 `version` 在每次发布时设为发布版本的 `major.minor.(patch+1)`：发布 `2.0.0-beta.1` 或 `2.0.0` 时都写 `2.0.1`。nightly 实际版本为 `2.0.1-alpha+<hash>`，大于 `2.0.0` 及其所有 prerelease。

影响：

| 位置         | 结果                                                                                                                                                                                   |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| App 更新器   | beta 切 nightly 时提示更新；nightly 切回 beta 不降级（`is_newer_release` 本来如此）                                                                                                    |
| 配置迁移     | nightly 按 2.0.1 计算，会提前执行挂在 2.0.1 的迁移                                                                                                                                     |
| Windows NSIS | nightly 的 NSIS 只写基础版本 `2.0.1`（`prepare-nightly.ts` 对 `--nsis` 不改版本）。这一行为必须保留：若写入 `-alpha+hash`，`SemverCompare` 会让两个 nightly 之间按 hash 字典序判升降级 |
| deb / rpm    | `2.0.1+alpha.<hash>` 大于 `2.0.0` ✓；nightly 之间的排序见 §7 Q1                                                                                                                        |

## 2. Publish 工作流（`.github/workflows/publish.yml`）

### 2.1 输入

- `versionType`：`major | minor | patch | premajor | preminor | prepatch | prerelease`，默认 `patch`。
- `preid`：`beta | rc`，默认 `beta`，只对 `pre*` 生效。

### 2.2 步骤

1. Checkout（`fetch-depth: 0`，需要全部 tag）。
2. 安装 Node、pnpm 依赖、Deno（现有工作流缺 Deno，而 `pnpm run publish` 实际执行 `deno run`；上次正式发布 v1.6.1 早于脚本迁移到 Deno）。
3. 运行版本相关测试：`publish_test.ts`、`prepare-release_test.ts`、`finalize-linux-release_test.ts`（ubuntu-latest 上用真实 dpkg）、`release-channel_test.ts`。
4. 计算并写入版本（§1.2、§1.3）；输出 `version` 与 `prerelease`（版本含 `-` 即为 true）。
5. 生成 changelog：
   - prerelease：`git-cliff --unreleased`，范围是上一个 tag（含 prerelease tag）到 HEAD；只放进 release 正文，不写 `CHANGELOG.md`。
   - 正式版：`git-cliff --ignore-tags 'v.*-.*' <上一个稳定 tag>..HEAD`，把期间各个 prerelease 的提交一并汇总，并写入 `CHANGELOG.md`。已在临时仓库用项目的 `cliff.toml` 验证；单用 `--unreleased --ignore-tags` 不会合并 prerelease 的提交。
6. 提交 `chore: bump version to vX` 并打 tag。
7. 创建草稿 release，`prerelease: ${{ steps.update-version.outputs.prerelease }}`。

发布草稿后触发 `release: published`（prerelease 同样触发），走 `target-release-build.yaml`。

## 3. Linux 打包（beta 与正式版构建）

### 3.1 `prepare-release.ts --linux`

正式构建（`nightly == false`）在 `deps-build-linux.yaml` 中、打包前执行。prerelease 时：

- `tauri.conf.json` 的 `version` 改为 `X.Y.Z`（去掉 prerelease）。
- `bundle.linux.rpm.release` 设为 `0.<prerelease>`，例如 `0.beta.1`。

正式版不做任何修改（rpm release 保持默认 `1`）。纯函数 `linuxPackageVersions(version)` 返回 `{ tauriVersion, rpmRelease, debVersion }`，例如 `2.0.0-beta.1` → `{ "2.0.0", "0.beta.1", "2.0.0~beta.1" }`；正式版 `2.0.0` → `{ "2.0.0", "1", "2.0.0" }`。

这会替换当前分支中写成 `2.0.0+beta.1` 的实现（§0.3 实测证明它排序错误）。

### 3.2 rpm

由 Tauri 原生生成，Version `2.0.0`、Release `0.beta.1`，文件名形如 `Clash Nyanpasu-2.0.0-0.beta.1.x86_64.rpm`，已能区分 beta。无需后处理。

### 3.3 deb：打包后改写 control

Tauri 无法单独设置 deb 版本，因此在 `tauri build` 之后、"Calc the archive signature" 之前，新增 `scripts/finalize-linux-release.ts`（x86_64 与 aarch64 cross 两条路径都执行）：

0. 先跑 `finalize-linux-release_test.ts`，在当前 runner 的 dpkg 上验证改写逻辑。
1. 对每个 `backend/target/**/bundle/deb/*.deb`：用 `ar` 取出 `control.tar.*`，把 `control` 中的 `Version:` 改为 `debVersion`，重新打包 control 成员并用 `ar r` 原位替换。`debian-binary` 与 `data.tar.*` 成员不动，因此 `md5sums` 仍然有效。
2. 文件名中的版本段替换为 release 版本，例如 `Clash Nyanpasu_2.0.0_amd64.deb` → `Clash Nyanpasu_2.0.0-beta.1_amd64.deb`（文件名里不使用 `~`，以免 GitHub 改写资源名）。
3. 校验：`dpkg-deb -f <file> Version` 必须等于 `debVersion`，否则失败。

正式版时 `debVersion` 等于 tauri 版本，脚本只做校验，不改写。

备选方案：`dpkg-deb -R` 解包后 `dpkg-deb --root-owner-group -b` 重打包。它会重压 `data.tar`，改动面更大，不采用。

### 3.4 AppImage

- AppImage 没有系统包版本号。打包版本是 `2.0.0`，App 内版本仍是 `2.0.0-beta.1`。
- `tauri build` 后无条件运行 `sign-nightly-updater.ts`，用 `package.json` 的版本重签 `*.AppImage.tar.gz`。正式版重签结果与原签名等价；prerelease 则把签名版本从 `2.0.0` 改为 `2.0.0-beta.1`，与清单 tag 一致（否则 `collectUpdaterPlatforms` 报签名版本不一致）。
- 文件名同步改成 release 版本（`.AppImage`、`.AppImage.tar.gz`、`.sig`），由 §3.3 的脚本在重签之后处理。

## 4. Windows 与 macOS

- **Windows NSIS**：不做改动。`prepare-release.ts --nsis` 维持现状（不改版本）。beta 升正式版判为升级；beta 覆盖 nightly 判为降级，不会被拦（`allowDowngrades` 默认 true）；更新器带 `/UPDATE` 时跳过判断。文件属性里的 `VIProductVersion` 对 beta 和正式版都是 `2.0.0.0`，只影响显示。
- **macOS**：不做改动。`CFBundleShortVersionString` 带 `-beta.1` 不影响非 App Store 分发，nightly 一直如此。未在真机验证。
- **MSI**：项目不打包 MSI，不涉及。

## 5. 发布后环节

- **Telegram 通知**（`scripts/telegram-notify.ts`）：非 nightly 时改为按 `releases/tags/v${version}` 取 release。原来用 `releases/latest`，GitHub 的这个接口永远不返回 prerelease，发 beta 时会拿到上一个正式版的资源。
- **更新清单**（`deps-create-updater.yaml`）：不改。`RELEASE_TAG` 等于 beta tag 时，beta 清单的说明用 release 正文；stable 清单仍指向最新正式版。
- **资源上传**（`deps-upload-release-assets.yaml`）：不改；它会拒绝重名资源，§3.3 的 deb 重命名不会与其他产物冲突。

## 6. 验证计划

| 项                                                                                | 方式                                                                                                                                                |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| 版本计算（预置清单起步、prerelease 迭代、换 preid、转正、nightly 基准、拒绝倒退） | `scripts/publish_test.ts`，纯函数测试                                                                                                               |
| Linux 版本映射                                                                    | `scripts/prepare-release_test.ts`，纯函数测试                                                                                                       |
| deb/rpm 排序                                                                      | 测试中固化 §0.3 的期望值；CI 的 Ubuntu 上用 `dpkg --compare-versions` 断言 `debVersion` 的序关系                                                    |
| deb control 改写                                                                  | 新测试：构造一个最小 deb，跑改写，断言 `dpkg-deb -f` 的 Version 与 `data.tar` 字节不变。在 Ubuntu CI 或 WSL 上运行                                  |
| changelog 范围                                                                    | 临时仓库验证（已完成），写入 spec 结论                                                                                                              |
| 端到端                                                                            | 在 fork 上运行一次 Publish（`premajor` + beta）并发布草稿，检查各平台产物名、`update-beta.json`、Telegram 取到的资源                                |
| 回归                                                                              | 现有 `updater-platforms_test.ts`、`sign-nightly-updater_test.ts`、`release-channel_test.ts` 通过；`pnpm lint:deno`、prettier、actionlint 不新增告警 |

## 7. 待决问题

- **Q1 nightly 在 deb/rpm 上的内部排序**（既有问题）：nightly 版本是 `X.Y.Z+alpha.<hash>`，hash 无序，nightly 之间用 apt/dnf 升级可能被判为降级。可选修法：Linux nightly 复用 §3 的机制，deb 用 `X.Y.Z~alpha.<UTC 时间戳>`，rpm 用 Version `X.Y.Z`、Release `0.alpha.<UTC 时间戳>`；App 内版本与签名仍是 `-alpha+<hash>`。时间戳版本仍小于 `X.Y.Z` 正式版、大于上一个正式版。建议：本次不做，单独跟进。
- **Q2 AppImage 文件名**：已裁定，一起改名（§3.4）。
- **Q3 自由输入的 `version` 参数**：已裁定，不加（§1.2）。

## 8. 证据

- NSIS 核验（子代理，2026-10-01）：
  - `tauri@tauri-cli-v2.12.0`：`crates/tauri-bundler/src/bundle/windows/nsis/mod.rs` L42-43、L149-171、L314-319；`installer.nsi` L43-44、L90-95、L225-263、L311-346、L528-541、L704。
  - `nsis-tauri-utils@nsis_tauri_utils-v0.5.3`：`crates/nsis-semvercompare/src/lib.rs` L16-47。
  - PR #12136（非数字构建元数据由报错改为警告）、#6120。
- deb/rpm 排序：WSL Ubuntu 上 `dpkg --compare-versions` 与 `rpm --eval '%{lua:print(rpm.vercmp(...))}'` 的输出（§0.3）。
- git-cliff 2.10.1：临时仓库中 `--ignore-tags 'v.*-.*' <stable>..HEAD` 能合并 prerelease 提交，`--unreleased --ignore-tags` 不能。

## 9. 实施状态（2026-10-02）

已按本设计实施：§1.2 的基准与防护、§2 的工作流（含 tag 已存在检查）、§3 的 rpm release、deb 改写与改名（`finalize-linux-release.ts`）、§5 的 Telegram 修正。`+prerelease` 的旧实现已移除。

验证：

- `publish_test.ts`（6 个）、`prepare-release_test.ts`（3 个）、`finalize-linux-release_test.ts`（3 个，其中 dpkg 集成测试在 WSL Ubuntu 的真实 dpkg 上通过：Version 改为 `2.0.0~beta.1`，`data.tar` 字节不变，`dpkg-deb --contents` 正常）。
- 从 `v1.6.1` 到当前 HEAD 的 prerelease changelog 约 86 KB，低于 GitHub release 正文 125,000 字符的上限，但正式版 `2.0.0` 的 changelog 范围相同且仍会增长，需要留意。
- 未验证：在 fork 上端到端运行 Publish 与发布构建。

## 10. Beta 频道更新链路审计（2026-10-01）

范围：从发布构建到 App 内检查、下载、安装的整条 beta 更新链路。已核对且无需修改的环节列在 §10.3。

### 10.1 发现与处理

| #   | 严重度 | 问题                                                                                                                                                                                                                                                                                                                                                                                                                                   | 处理                                                                                                                                                               |
| --- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A1  | 阻断   | `target-release-build.yaml` 的 `updater` job 调用 `deps-create-updater.yaml` 时没有传 secrets，而后者要求 `SURGE_TOKEN`（actionlint 报错）。发布构建跑到更新清单这一步会失败，beta 清单 `update-beta.json` 无法生成                                                                                                                                                                                                                    | 已修：加 `secrets: inherit`（与 nightly 的 `target-dev-build.yaml` 一致）                                                                                          |
| A2  | 高     | 发布构建只有 Windows x86_64 与 Linux x86_64，缺少 nightly 已有的 Windows aarch64 与 Linux aarch64。beta/正式版没有 ARM 安装包，清单里也没有 `windows-aarch64`、`windows-aarch64-fixed-webview`                                                                                                                                                                                                                                         | 已修：补两个 job，并加入 `upload_release_assets` 的 `needs`                                                                                                        |
| A3  | 高     | Linux aarch64 的正式构建命令 `-c "{ "bundle": ... }"` 引号嵌套错误，shell 实际传给 Tauri 的是非 JSON 的 `{ bundle: { createUpdaterArtifacts: false } }`（已用 bash 复现）。该路径此前从未运行                                                                                                                                                                                                                                          | 已修：改为单引号包裹的合法 JSON                                                                                                                                    |
| A4  | 高     | 前端 `isSupported = isTauri() && (!isAppImage \|\| !WIN_PORTABLE)` 是 `5730f37f6` 简化时引入的回归：<br>① Linux deb/rpm 安装（非 AppImage）被判为支持。updater 2.13 对 deb/rpm 包先找 `linux-x86_64-deb`/`-rpm`，找不到就回落到 `linux-x86_64`，拿到 AppImage 包后 `install_deb` 校验格式失败。<br>② `WIN_PORTABLE` 是编译期常量，而 portable zip 打包的是与安装版同一次编译的 exe，所以它恒为 false，便携版会被提示用 NSIS 安装器更新 | 已修：新增 `useIsPortable`（后端已有 `is_portable` RPC，依据 `.config/PORTABLE` 标记），支持判断提为纯函数 `utils/updater-support.ts::isUpdaterSupported` 并加单测 |
| A5  | 中     | 多个发布（例如 beta 与正式版接连发布）的更新清单 job 会并发地删除并重新上传 `updater` release 上的同一批资源，可能出现 422 或清单短暂缺失                                                                                                                                                                                                                                                                                              | 已修：`deps-create-updater.yaml` 按 nightly/非 nightly 分组串行（`cancel-in-progress: false`）。同组较新的排队任务替换较旧的，因为每次都从当前 release 列表重建    |
| A6  | 待决   | nightly 不可逆的副作用：用户一旦选过 Nightly，即使手动安装 beta 或正式版，启动时 `build_channel.resolve(Some(Nightly))` 仍得到 Nightly 并被持久化，`validate_channel` 又拒绝离开 Nightly。结果是这个 beta 构建会立即提示"更新"到 nightly（`2.0.1-alpha` 大于 `2.0.0-beta.1`）。对于 2.0 期间一直用 nightly 的测试者，手动换装 beta 后会被拉回 nightly                                                                                  | 未改。需要决定：保持现状（只能删配置脱离），或者让"安装的构建是非 nightly 时允许离开 Nightly"                                                                      |
| A7  | 待决   | deb/rpm 安装无法 App 内更新（A4 修复后改为引导到 GitHub Releases）。若要支持，需要发布 deb/rpm 的签名更新包并在清单里加 `linux-x86_64-deb`、`linux-x86_64-rpm` 目标，且安装需要提权                                                                                                                                                                                                                                                    | 未改，建议单独跟进                                                                                                                                                 |

### 10.2 验证

- actionlint：`target-release-build.yaml` 无告警；`deps-build-linux.yaml`、`deps-create-updater.yaml` 只剩原有告警（required 输入带默认值、`ubuntu-26.04` 标签）。
- 用真实 GitHub release 只读跑 `selectChannelRelease` 与 `collectUpdaterPlatforms`：当前 stable 与 beta 都选中 `v1.6.1`，平台收集成功，没有签名版本冲突，没有重复目标。
- `tests/updater-support.test.ts`（7 个用例）、`update-source-selector`、`interface/tests/release-channel` 通过；`pnpm lint:ts`、oxlint、prettier 通过。
- 未验证：发布构建的端到端运行（需在 fork 上触发）；真机上的 Windows ARM、Linux aarch64 安装包。

### 10.3 已核对、无需修改

- 后端频道判定、端点（`update-beta{,-proxy}.json`）、`is_newer_release`（Stable 拒绝 prerelease；Beta 跟随含正式版在内的最高版本；切回 Stable 不降级）。
- 下载地址校验 `update_download_urls`：只限定 host 与 `/libnyanpasu/clash-nyanpasu/releases/download/` 前缀，beta tag 路径可通过。
- 清单：updater 插件读取 `name` 并去掉前导 `v`（nightly 一直如此运行）；签名版本校验由 §3.4 的重签覆盖 Linux，Windows/macOS 打包版本即 release 版本。
- 配置迁移按 (major, minor, patch) 判定，beta 会执行 2.0.0 的迁移。
- macOS 更新包改名（`upload-macos-updater.ts`）与版本无关；Windows 安装包与 fixed-webview 变体在 beta 中都有。
- Telegram 按 tag 取 release（§5）。`UPDATELOG.md` 找不到 prerelease 段落时回落到 release 正文。

### 10.4 裁定与实施（2026-10-01）

- **A6：只要安装的不是 nightly 构建，就允许离开 Nightly。**
  - `ApplicationActor::validate_channel` 改为按安装构建的频道（`ApplicationActorArgs::build_channel`）判断，不再按持久化的偏好判断。`ConfigError::LeaveNightlyChannel` 的注释"A nightly build keeps its channel"本来就是这个意图。
  - 启动时不自动重置：非 nightly 构建装在 Nightly 偏好之上时，继续跟随 Nightly，直到用户在设置里切走。这样保留了"刚选 Nightly、尚未装上 nightly 构建"的情况。
  - `get_release_channel` 改为返回 `ReleaseChannelInfo { current, installed }`。前端按 `installed === 'nightly'` 锁定选择器，Nightly 提示文案改为"每夜版构建无法切回稳定版或测试版"（5 种语言）。
  - 测试：原 `release_channel_persists_and_cannot_leave_nightly` 断言非 nightly 构建也不能离开，与新规则相反，已拆为 `release_channel_persists_and_non_nightly_builds_can_leave_nightly` 和 `nightly_build_cannot_leave_nightly`。
- **A7：不支持 deb/rpm 的应用内更新。** A4 修复后，这两种安装方式改为引导到 GitHub Releases，不再做别的改动。

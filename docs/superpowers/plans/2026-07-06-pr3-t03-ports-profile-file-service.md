# PR-3 T03 — ports + ProfileFileService Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地消费方拥有的三个窄 trait(`ProfileFsPort`/`SubscriptionFetcher`/`RebuildNotifier`,Tauri-free、mockall 兼容)与具体实现 `ProfileFileService`(fs 原子写、符号链接防御、reqwest 订阅下载);纯增量、零调用方。

**Architecture:** trait 定义在消费方模块 `state/profiles/ports.rs`(D10 边界);实现在 `service/profile_file.rs`,持 `PathResolver`(相对 `ManagedProfilePath` → `app_profiles_dir()` 解析)与注入的 `SelfProxyPortSource`(self_proxy 端口来源,替代 legacy 直读 `Config::verge()` 的 FIXME,remote.rs:130-136)。下载语义 1:1 移植 legacy `subscribe_url`(代理优先级/UA/hwid 头/backon 重试/subscription-userinfo 解析),**不做内容格式校验**(按目标类型的校验属于 T05 下载任务,design 图 13.3)。

**Tech Stack:** async-trait 0.1(已有)、mockall 0.13(**本卡加入 dev-deps**)、atomicwrites 0.4(已有)、reqwest(workspace)、backon(已有)、sysproxy(已有)、axum 0.8(已有,测试服务器)、tempfile(已有)。

## Global Constraints(task.md §0)

- `state/profiles/**` 禁止 `tauri::*` / `crate::config` import(grep 断言);`service/profile_file.rs` 允许 fs/网络,同样禁止 `tauri::*` 与 legacy `Config`。
- ports 兼容 `mockall::automock`;测试不 sleep(轮询/超时用 tokio 时间与有界等待)。
- 模块布局(plan 预决策):`state/profiles/{mod.rs, ports.rs}`(actor.rs/scheduler.rs 留给 T04/T05)。
- 每个 commit `cargo build` + `cargo test` 绿。

## 基线事实(2026-07-06 实测)

- `PathResolver::with_base_dirs(config, data)` 可注入测试根(`utils/path.rs:60`);`app_profiles_dir() = config_dir/profiles`(`:94`)、`profiles_path()`(`:99`)。
- `backend/tauri/src/service/` 不存在——本卡新建;`state/` 现为平铺文件(application.rs 等),本卡引入 `profiles/` 子目录。
- legacy 下载语义源 `config/profile/item/remote.rs:118-215`:30s 超时、`self_proxy` 优先于 `with_proxy`、系统代理经 `Sysproxy::get_system_proxy()`、UA 默认 `clash-nyanpasu/v{APP_VERSION}`、hwid 四头、backon 指数重试(401/403/404 不重试)、`subscription-userinfo` 头解析、`profile-title`(支持 `base64:` 前缀)/`Content-Disposition` 文件名解析。
- mockall 全 workspace 未引入;async trait 的 automock 需 `#[cfg_attr(test, mockall::automock)]` 位于 `#[async_trait::async_trait]` **之上**。

## 契约补遗(执行后回写 T03 卡,§5.3)

1. `FetchedSubscription` 增加 `filename: Option<String>`(legacy 用响应头命名导入的 profile,T08 `import_profile` 需要)。
2. 新增 service 侧 trait `SelfProxyPortSource { fn mixed_port(&self) -> Option<u16> }`——`ProfileFileService::new(paths, self_proxy_port)` 取代卡内草签 `new(paths, http: reqwest::Client)`(reqwest 代理是 per-client 配置,必须按 options 逐次建 client,固定 Client 参数无法表达)。
3. `ProfileFsPort::remove` 语义 = 幂等(文件缺失也 Ok)。

---

### Task 1: 依赖 + 模块骨架 + ports 定义

**Files:**

- Modify: `backend/tauri/Cargo.toml`(`[dev-dependencies]` 追加 `mockall = "0.13"`)
- Create: `backend/tauri/src/state/profiles/mod.rs`
- Create: `backend/tauri/src/state/profiles/ports.rs`
- Modify: `backend/tauri/src/state/mod.rs`(追加 `pub mod profiles;`)
- Create: `backend/tauri/src/service/mod.rs`
- Modify: `backend/tauri/src/lib.rs`(在现有 `mod state;` 声明附近追加 `mod service;`;先 `grep -n "^mod \|^pub mod " backend/tauri/src/lib.rs` 定位声明区)

**Interfaces:**

- Produces(T04/T05/T07 依赖,签名冻结):

```rust
// state/profiles/ports.rs
pub trait ProfileFsPort: Send + Sync + 'static { read / write_atomic / remove / ensure_not_symlink / ensure_symlink }
pub trait SubscriptionFetcher: Send + Sync + 'static { async fetch(&self, &Url, &RemoteProfileOptions) -> anyhow::Result<FetchedSubscription> }
pub trait RebuildNotifier: Send + Sync + 'static { request_rebuild(&self) }
pub struct FetchedSubscription { content: String, filename: Option<String>, subscription: SubscriptionInfo }
```

- [ ] **Step 1: Cargo dev-dep**

`backend/tauri/Cargo.toml` 的 `[dev-dependencies]` 段追加一行:

```toml
mockall = "0.13"
```

- [ ] **Step 2: 写 `state/profiles/mod.rs`**

```rust
//! Profiles domain state (PR-3). Actor lands in `actor.rs` (T04), background
//! scheduler in `scheduler.rs` (T05); this module stays Tauri-free (D10).

pub mod ports;
```

- [ ] **Step 3: 写 `state/profiles/ports.rs`(完整内容)**

```rust
//! Consumer-owned ports for the profiles actor (design §7, D10). Concrete
//! implementations live in `crate::service::profile_file`.

use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, RemoteProfileOptions, SubscriptionInfo,
};
use url::Url;

/// Filesystem access for materialized profile files. Paths are relative to the
/// app profiles dir; resolution is the implementation's concern.
#[cfg_attr(test, mockall::automock)]
pub trait ProfileFsPort: Send + Sync + 'static {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String>;
    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()>;
    /// Idempotent: removing a missing file succeeds.
    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    /// Remote-updater write guard: the target must not be an unexpected
    /// symlink (clean-design §9 last paragraph).
    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    /// Create or repair `path -> target` (External Symlink binding, clean-design §10.1).
    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct FetchedSubscription {
    pub content: String,
    /// Server-provided display name (`profile-title` / `Content-Disposition`).
    pub filename: Option<String>,
    pub subscription: SubscriptionInfo,
}

/// Subscription download. Network timeouts are managed inside the
/// implementation (D9); content validation is the caller's concern (per
/// target profile kind, design fig. 13.3).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait SubscriptionFetcher: Send + Sync + 'static {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription>;
}

/// Background-commit rebuild signal (design §6.4). Fire-and-forget; debouncing
/// is the receiver's concern.
#[cfg_attr(test, mockall::automock)]
pub trait RebuildNotifier: Send + Sync + 'static {
    fn request_rebuild(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_mockable_and_object_safe() {
        let _fs: Box<dyn ProfileFsPort> = Box::new(MockProfileFsPort::new());
        let _fetcher: Box<dyn SubscriptionFetcher> = Box::new(MockSubscriptionFetcher::new());
        let _notifier: Box<dyn RebuildNotifier> = Box::new(MockRebuildNotifier::new());
    }
}
```

- [ ] **Step 4: 写 `service/mod.rs` + 模块声明**

```rust
//! Concrete infrastructure services (adapters) consumed via ports.

pub mod profile_file;
```

`state/mod.rs` 追加 `pub mod profiles;`;`lib.rs` 声明区追加 `mod service;`。`service/profile_file.rs` 本 Task 先建占位空文件(Task 2 填充):

```rust
//! ProfileFsPort + SubscriptionFetcher implementation (T03 Task 2/3).
```

- [ ] **Step 5: 编译 + 纯度断言 + mock 冒烟**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu ports_are_mockable`
Expected: PASS。
Run(Git Bash): `grep -rn "tauri::\|crate::config" backend/tauri/src/state/profiles/ && echo LEAK || echo CLEAN`
Expected: `CLEAN`。

- [ ] **Step 6: Commit**

```bash
git add backend/tauri/Cargo.toml backend/Cargo.lock backend/tauri/src/state/profiles backend/tauri/src/state/mod.rs backend/tauri/src/service backend/tauri/src/lib.rs
git commit -m "feat(tauri): add profile fs/subscription/rebuild ports (PR-3 T03)"
```

---

### Task 2: `ProfileFileService` 文件系统半(ProfileFsPort impl)

**Files:**

- Modify: `backend/tauri/src/service/profile_file.rs`

**Interfaces:**

- Produces: `ProfileFileService::new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>) -> Self`(同时 impl `ProfileFsPort` + `SubscriptionFetcher`)、`pub trait SelfProxyPortSource { fn mixed_port(&self) -> Option<u16>; }`、`pub fn normalize_yaml_document(content: &str) -> anyhow::Result<String>`。

- [ ] **Step 1: 写失败测试(tempdir)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::profile::ManagedProfilePath;
    use std::sync::Arc;

    struct NoProxy;
    impl SelfProxyPortSource for NoProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (temp, ProfileFileService::new(paths, Arc::new(NoProxy)))
    }

    fn managed(name: &str) -> ManagedProfilePath {
        ManagedProfilePath::new(name).unwrap()
    }

    #[test]
    fn write_atomic_then_read_round_trips_and_creates_parent() {
        let (_temp, service) = service();
        let path = managed("abc.yaml");
        service.write_atomic(&path, "proxies: []\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "proxies: []\n");
        // overwrite keeps working (AllowOverwrite)
        service.write_atomic(&path, "mode: rule\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "mode: rule\n");
    }

    #[test]
    fn remove_is_idempotent() {
        let (_temp, service) = service();
        let path = managed("gone.yaml");
        service.remove(&path).unwrap(); // missing → Ok
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.remove(&path).unwrap();
        assert!(service.read(&path).is_err());
    }

    #[test]
    fn ensure_not_symlink_rejects_links_and_accepts_files() {
        let (_temp, service) = service();
        let path = managed("real.yaml");
        service.ensure_not_symlink(&path).unwrap(); // missing → Ok
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.ensure_not_symlink(&path).unwrap(); // regular file → Ok

        // 构造符号链接;无权限环境(Windows 非开发者模式)跳过
        let link = managed("link.yaml");
        let target_file = service.resolve(&path);
        let link_file = service.resolve(&link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target_file, &link_file);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target_file, &link_file);
        if made.is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert!(service.ensure_not_symlink(&link).is_err());
    }

    #[test]
    fn ensure_symlink_creates_and_repairs() {
        let (temp, service) = service();
        let outside = temp.path().join("outside.yaml");
        std::fs::write(&outside, "external: true\n").unwrap();
        let target =
            nyanpasu_config::profile::ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        let link = managed("ext.yaml");
        if service.ensure_symlink(&link, &target).is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert_eq!(service.read(&link).unwrap(), "external: true\n");
        // 幂等:重复调用 Ok
        service.ensure_symlink(&link, &target).unwrap();
        // 既存普通文件 → 拒绝
        let occupied = managed("occupied.yaml");
        service.write_atomic(&occupied, "x: 1\n").unwrap();
        assert!(service.ensure_symlink(&occupied, &target).is_err());
    }

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu profile_file`
Expected: FAIL(类型未定义)。

- [ ] **Step 3: 实现(文件系统半 + 服务骨架)**

```rust
//! ProfileFsPort + SubscriptionFetcher over the real filesystem, reqwest and
//! the OS proxy state (design §7). Tauri-free and legacy-Config-free; the
//! self-proxy port arrives via [`SelfProxyPortSource`] instead of the legacy
//! global read (config/profile/item/remote.rs:130-136 FIXME).

use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, RemoteProfileOptions, SubscriptionInfo,
};
use url::Url;

use crate::{
    state::profiles::ports::{FetchedSubscription, ProfileFsPort, SubscriptionFetcher},
    utils::path::PathResolver,
};

/// Where the fetcher looks up the app's own mixed port when `self_proxy` is
/// requested. Wired at the composition root (T07); tests inject a constant.
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}

pub struct ProfileFileService {
    paths: PathResolver,
    self_proxy_port: Arc<dyn SelfProxyPortSource>,
    http_timeout: Duration,
}

impl ProfileFileService {
    pub fn new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>) -> Self {
        Self {
            paths,
            self_proxy_port,
            http_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    fn resolve(&self, path: &ManagedProfilePath) -> PathBuf {
        self.paths.app_profiles_dir().join(path.as_path())
    }
}

/// Parse → reserialize a YAML mapping so editor saves and File-config reads
/// share one canonical shape (legacy `read_profile_file` normalization).
pub fn normalize_yaml_document(content: &str) -> anyhow::Result<String> {
    let mapping: serde_yaml::Mapping =
        serde_yaml::from_str(content).context("document is not a YAML mapping")?;
    serde_yaml::to_string(&mapping).context("failed to reserialize YAML mapping")
}

impl ProfileFsPort for ProfileFileService {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String> {
        let full = self.resolve(path);
        std::fs::read_to_string(&full)
            .with_context(|| format!("read profile file {}", full.display()))
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        AtomicFile::new(&full, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("atomic write {}", full.display()))
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("remove profile file {}", full.display())),
        }
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => bail!(
                "refusing to write through unexpected symlink at {}",
                full.display()
            ),
            _ => Ok(()),
        }
    }

    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if std::fs::read_link(&full)? == target.as_path() {
                    return Ok(());
                }
                std::fs::remove_file(&full)?;
            }
            Ok(_) => bail!(
                "existing non-symlink file at {}, refusing to replace",
                full.display()
            ),
            Err(_) => {}
        }
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target.as_path(), &full)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target.as_path(), &full)?;
        Ok(())
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu profile_file`
Expected: 除 fetch 相关(尚未写)外全 PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/service/profile_file.rs
git commit -m "feat(tauri): implement profile file service filesystem port"
```

---

### Task 3: `SubscriptionFetcher` 网络半(legacy 语义 1:1)

**Files:**

- Modify: `backend/tauri/src/service/profile_file.rs`

- [ ] **Step 1: 写失败测试(axum 本地服务器,零新依赖)**

```rust
    // ---- fetch tests --------------------------------------------------------
    use axum::{Router, http::HeaderMap as AxumHeaderMap, routing::get};
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}/"), handle)
    }

    fn options_direct() -> RemoteProfileOptions {
        RemoteProfileOptions {
            user_agent: None,
            with_proxy: false,
            self_proxy: false,
            update_interval_minutes: 120,
        }
    }

    #[tokio::test]
    async fn fetch_parses_userinfo_and_title_headers() {
        let router = Router::new().route(
            "/",
            get(|| async {
                let mut headers = AxumHeaderMap::new();
                headers.insert(
                    "subscription-userinfo",
                    "upload=1; download=2; total=3; expire=0".parse().unwrap(),
                );
                headers.insert("profile-title", "My Sub".parse().unwrap());
                (headers, "proxies: []\n")
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "proxies: []\n");
        assert_eq!(fetched.filename.as_deref(), Some("My Sub"));
        assert_eq!(fetched.subscription.upload, Some(1));
        assert_eq!(fetched.subscription.download, Some(2));
        assert_eq!(fetched.subscription.total, Some(3));
        assert!(fetched.subscription.expire.is_none()); // expire=0 → None
    }

    #[tokio::test]
    async fn fetch_retries_transient_errors_but_not_auth_failures() {
        static HITS: AtomicUsize = AtomicUsize::new(0);
        let flaky = Router::new().route(
            "/",
            get(|| async {
                if HITS.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                } else {
                    Ok("ok: true\n")
                }
            }),
        );
        let (url, _server) = serve(flaky).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "ok: true\n");
        assert!(HITS.load(Ordering::SeqCst) >= 2);

        static AUTH_HITS: AtomicUsize = AtomicUsize::new(0);
        let forbidden = Router::new().route(
            "/",
            get(|| async {
                AUTH_HITS.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::FORBIDDEN
            }),
        );
        let (url, _server) = serve(forbidden).await;
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert_eq!(AUTH_HITS.load(Ordering::SeqCst), 1); // 403 不重试
    }

    #[tokio::test]
    async fn fetch_timeout_is_managed_internally() {
        let slow = Router::new().route(
            "/",
            get(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                "never"
            }),
        );
        let (url, _server) = serve(slow).await;
        let (_temp, service) = service();
        let service = service.with_http_timeout(std::time::Duration::from_millis(300));
        let started = std::time::Instant::now();
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
```

(注:`service()` 返回的 `_temp` 守卫必须持有到测试结束;`RemoteProfileOptions` 字段名以 `nyanpasu-config/src/profile/source.rs:109-122` 为准。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu fetch_`
Expected: FAIL(`SubscriptionFetcher` 未实现)。

- [ ] **Step 3: 实现网络半**

```rust
#[async_trait::async_trait]
impl SubscriptionFetcher for ProfileFileService {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription> {
        use backon::Retryable;

        let mut builder = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .no_proxy()
            .timeout(self.http_timeout);

        // Proxy precedence mirrors the legacy subscriber (remote.rs:129-150):
        // self_proxy wins, then system proxy, else direct.
        let proxy_url = if options.self_proxy {
            self.self_proxy_port
                .mixed_port()
                .map(|port| format!("http://127.0.0.1:{port}"))
        } else if options.with_proxy {
            match sysproxy::Sysproxy::get_system_proxy() {
                Ok(p @ sysproxy::Sysproxy { enable: true, .. }) => {
                    Some(format!("http://{}:{}", p.host, p.port))
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(proxy_url) = proxy_url {
            use crate::utils::config::NyanpasuReqwestProxyExt;
            builder = builder.swift_set_proxy(&proxy_url);
        }

        let user_agent = options.user_agent.clone().unwrap_or_else(|| {
            format!("clash-nyanpasu/v{}", crate::utils::dirs::APP_VERSION)
        });
        let client = builder.user_agent(user_agent).build()?;

        let device_info = crate::utils::hwid::get_device_info();
        let sanitize = crate::utils::hwid::sanitize_for_header;
        let perform = || async {
            client
                .get(url.as_str())
                .header("x-hwid", &device_info.hwid)
                .header("x-device-os", sanitize(&device_info.device_os))
                .header("x-ver-os", sanitize(&device_info.os_version))
                .header("x-device-model", sanitize(&device_info.device_model))
                .send()
                .await?
                .error_for_status()
        };
        let resp = perform
            .retry(backon::ExponentialBuilder::default())
            .when(|error: &reqwest::Error| {
                !error.is_status()
                    || error.status().is_some_and(|status| {
                        !matches!(
                            status,
                            reqwest::StatusCode::FORBIDDEN
                                | reqwest::StatusCode::NOT_FOUND
                                | reqwest::StatusCode::UNAUTHORIZED
                        )
                    })
            })
            .await
            .with_context(|| format!("subscription download failed: {url}"))?;

        let subscription = parse_subscription_userinfo(resp.headers());
        let filename = parse_profile_title(resp.headers());
        let content = resp.text().await?;
        Ok(FetchedSubscription {
            content,
            filename,
            subscription,
        })
    }
}

/// `subscription-userinfo: upload=..; download=..; total=..; expire=..` →
/// typed info. Missing header → empty; `expire=0` → None (matches migration R6).
fn parse_subscription_userinfo(headers: &reqwest::header::HeaderMap) -> SubscriptionInfo {
    let Some(value) = headers
        .get("subscription-userinfo")
        .or_else(|| headers.get("Subscription-Userinfo"))
    else {
        return SubscriptionInfo::default();
    };
    let raw = value.to_str().unwrap_or("");
    let field = |key: &str| crate::utils::help::parse_str::<u64>(raw, key);
    SubscriptionInfo {
        upload: field("upload"),
        download: field("download"),
        total: field("total"),
        expire: field("expire")
            .filter(|secs| *secs != 0)
            .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs as i64).ok()),
    }
}

/// `profile-title`(支持 `base64:` 前缀)→ `Content-Disposition filename=` 兜底
/// (legacy remote.rs:213-215 + item/remote.rs `parse_profile_title_header`)。
fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("profile-title").and_then(|v| v.to_str().ok()) {
        if let Some(encoded) = value.strip_prefix("base64:") {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    return Some(decoded);
                }
            }
        } else if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split("filename=").nth(1))
        .map(|s| s.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|s| !s.is_empty())
}
```

(若 `time` crate 未在 tauri 依赖中:`grep -n "^time" backend/tauri/Cargo.toml`;缺失则通过 `nyanpasu_config` 重导出或在 Cargo.toml 加 `time = { version = "0.3", features = ["serde"] }`——以编译错误为准,最小改动。)

- [ ] **Step 4: 跑全部 T03 测试**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu profile_file fetch_ ports_are_mockable`
Expected: 全 PASS。

- [ ] **Step 5: 纯度终检 + Commit**

Run(Git Bash): `grep -rn "tauri::\|crate::config::" backend/tauri/src/service/profile_file.rs backend/tauri/src/state/profiles/ && echo LEAK || echo CLEAN`
Expected: `CLEAN`。

```bash
git add backend/tauri/src/service/profile_file.rs
git commit -m "feat(tauri): implement subscription fetcher with legacy download semantics"
```

---

### Task 4: 契约回写(task.md §5.3)

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T03 卡 Interfaces — Produces 段)

- [ ] **Step 1: 按实际落地更新 T03 卡**

1. `FetchedSubscription` 增加 `filename: Option<String>` 字段(T08 `import_profile` Consumes);
2. `ProfileFileService::new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>)` 替换草签的 `new(paths, http: reqwest::Client)`,并追记 `SelfProxyPortSource` trait(T07 在 composition root 提供实现);
3. `ProfileFsPort::remove` 标注幂等语义;
4. 追记 `normalize_yaml_document` 辅助函数(T07 `read_profile_file`/`save_profile_file` Consumes)。

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T03 contract addenda (fetcher signature, filename)"
```

---

## 验证总表(对应 T03 卡验证段)

| 判据                                     | 覆盖                                         |
| ---------------------------------------- | -------------------------------------------- |
| 原子写(tempdir)                          | Task 2 `write_atomic_then_read_...`          |
| `ensure_not_symlink` 拒绝符号链接        | Task 2 `ensure_not_symlink_rejects_...`      |
| YAML 规范化读                            | Task 2 `normalize_yaml_document_...`         |
| fetch 网络超时自管                       | Task 3 `fetch_timeout_is_managed_internally` |
| 重试语义(401/403/404 不重试)             | Task 3 `fetch_retries_transient_...`         |
| `state/profiles/` 无 tauri/config import | Task 1 Step 5 + Task 3 Step 5 grep           |
| ports 兼容 mockall                       | Task 1 `ports_are_mockable_and_object_safe`  |

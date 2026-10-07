# Proxies WebUI Tools (Phase C) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve latency tests against each group's test URL with a configurable timeout and `expected` status, read delays per URL, add search/sort/hide-unavailable to the group page with two new MD3 components, trim the `extra` payload, and add the group header chain/availability line, a three-mode break setting and a provider health check.

**Architecture:** The frontend resolves a group's effective test URL with pure functions in `@nyanpasu/query` (`proxy-delay.ts`) and passes it explicitly; the backend only falls back to the app config (`default_latency_test`, `default_latency_timeout_ms`) when no URL is given. Node-list preferences live in the backend KV store (`useKvStorage`). Trimming happens on the `NyanpasuClient` RPC output only; the `ProxiesActor` snapshot stays complete. The provider health check reuses the actor's provider-mutation path.

**Tech Stack:** Rust (nyanpasu-config, ractor actor, axum test fixtures, specta), TypeScript/React (TanStack Query/Router/Virtual, Radix via `@nyanpasu/ui`, Vitest unit + browser tests, Paraglide).

**Spec:** `docs/spec/2026-10-06-proxies-webui-tools/design.md`

## Global Constraints

- Work in the main checkout `/Users/a632079/Programs/clash-nyanpasu`. Stacked branches, each created from the previous phase's head after that phase passes review:
  - C1 `feat/proxies-latency-test-url` (exists; from `feat/proxies-pinned-selection`, carries the spec and this plan);
  - C2 `feat/proxies-node-list-tools`;
  - C3 `perf/proxies-trim-extra`;
  - C4 `feat/proxies-group-header-p2`.
- Follow `AGENTS.md` and `docs/development/*.md`. No new globals; no new `tauri` dependency in `NyanpasuClient`, typed clients, actors, or pure services.
- **All durable data is stored by the backend** (app config or `useKvStorage`). The search term in the URL `q` is transient view state.
- Do not hand-edit generated files (`frontend/rpc/src/rpc-bindings.ts`, `frontend/rpc/src/tauri-bindings.ts`, `frontend/query/src/query-bindings.ts`, `frontend/nyanpasu/src/paraglide/**`). Regenerate bindings with `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`. Paraglide output is gitignored; regenerate it with `pnpm web:build` before running frontend tests that use new keys.
- Rust tests: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib <filter>`; nyanpasu-config tests: `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-config <filter>`. Frontend tests from the repo root: `pnpm exec vitest run <file>`.
- Known pre-existing failures to ignore: Rust `connection_policy::profile_policy_and_noop_gates_do_not_acquire_a_source`; 15 `mockIPC`-based frontend browser test files that fail on `main` (`isTauri()` false under `@tauri-apps/api` 2.12.1 mocks). New tests must not use `mockIPC`; use `createTestRpc` (`frontend/query/tests/rpc-test-utils.tsx`) or `vi.mock('@nyanpasu/query', …)`.
- Every commit builds and passes `pnpm typecheck` on its own. Stage explicit paths only (never `git add .`/`-A`). One commit per task with the exact subject given; body lines ≤ 100 chars (commitlint). No `Co-Authored-By` trailers. A fix-up after review is folded into the task's commit (`git commit --fixup` + `GIT_SEQUENCE_EDITOR=: git rebase --autosquash -i <base>`), never a separate commit.
- New app config fields need `#[serde(default = …)]`: the migration check parses the full `NyanpasuAppConfig`, and a missing default crashes existing installs.
- Constants (verbatim): `DEFAULT_LATENCY_TIMEOUT_MS = 5000`; timeout range `1000..=30000` ms; fallback URL `http://www.gstatic.com/generate_204`; KV key `proxies-node-view`; default view `{ sort: 'default', hideUnavailable: false }`; sorts `'default' | 'name' | 'delay'`.
- UI follows Material You and the project's tokens (`primary`, `surface`, `surface-variant`, `on-surface`, `outline-variant`, `secondary-container`, …); keep dark mode, focus, disabled and keyboard behavior; `radix-ui` imports only inside `frontend/ui/`. Name roots and meaningful parts with `data-slot`. Separate responsibilities with one blank line.
- i18n: add keys to all five locales (`en`, `ko`, `ru`, `zh-cn`, `zh-tw`) at the same position. Proxies page keys go right after `proxies_clear_fixed_failed_message`; latency settings keys right after `settings_clash_settings_field_filter_label`; break-mode keys replace `settings_nyanpasu_enhance_break_when_proxy_change_*` in place; provider keys right after `providers_update_provider`. Write natural translations; English and Simplified Chinese wording is given per task.

## Review Focus

- A Clash-rs (`extra: {}`) or Meow (no `extra`) node: delays fall back to `history`, nothing shows "untested" wrongly. Pinned in Task 2 (`nodeDelayHistory` cases for `{}` and missing `extra`).
- A nested group member whose chain cycles (`A.now = B`, `B.now = A`) or points to a missing node: no hang, no crash, delay `undefined`. Pinned in Task 2 (`resolveChain` cycle and missing cases).
- A user who clears the default test URL setting, or saves a timeout outside 1–30 s: tests still use gstatic; the save is rejected with a visible error. Pinned in Task 1 (`delay_query` empty default; `validate_latency_timeout`) and Task 4.
- A search with extra spaces, mixed case or CJK; a hidden current node: matching trims and lowercases, the current node never disappears under "hide unavailable". Pinned in Task 6 (`matchesNodeSearch`, `filterMembers` keeps `now`).
- A group whose every member is filtered out, or whose current node is filtered out: empty-state text shows, the locate button is disabled rather than scrolling to a wrong index. Pinned in Task 7 browser test.

---

## Phase C1 — `feat/proxies-latency-test-url`

### Task 1: Backend latency test URL, timeout and `expected`

**Files:**

- Modify: `backend/nyanpasu-config/src/application/mod.rs` (field, default, validator, tests)
- Modify: `backend/tauri/src/state/config_error.rs` (variant)
- Modify: `backend/tauri/src/state/application.rs` (`commit` validation)
- Modify: `backend/tauri/src/client/application_workflow/impact.rs` (`app_cases`)
- Modify: `backend/tauri/src/core/clash/proxies.rs` (`ProxyGroup` fields + test)
- Modify: `backend/tauri/src/client/clash_api.rs` (`delay_query`, `proxy_delay`, `group_delay`, tests)
- Modify: `backend/tauri/src/ipc.rs` (`clash_api_get_proxy_delay`, `clash_api_get_group_delay`)
- Regenerate: `frontend/rpc/src/rpc-bindings.ts`, `frontend/rpc/src/tauri-bindings.ts`, `frontend/query/src/query-bindings.ts`
- Modify (compile fixes only): `frontend/query/src/ipc/use-clash-proxies.ts`, every TS test fixture that builds a `ProxyGroup` literal (`grep -rln "capabilities: {" frontend/*/tests`)

**Interfaces:**

- Produces (Rust): `ProxyGroup { test_url: Option<String>, expected_status: Option<String>, .. }`; `NyanpasuAppConfig::default_latency_timeout_ms: u64`; `nyanpasu_config::application::{DEFAULT_LATENCY_TIMEOUT_MS, validate_latency_timeout}`; `ConfigError::InvalidLatencyTimeout { reason }`; `NyanpasuClient::proxy_delay(name, provider, url, expected)`, `group_delay(group, url, expected)`.
- Produces (TS bindings): `ProxyGroup.testUrl: string | null`, `ProxyGroup.expectedStatus: string | null`; `NyanpasuAppConfig_Serialize.default_latency_timeout_ms: number`; `api.queries.clashApiGetProxyDelay(name, provider, url, expected)`; `api.queries.clashApiGetGroupDelay(group, url, expected)`.

- [ ] **Step 1: Config field, default and validator (test first)**

In `backend/nyanpasu-config/src/application/mod.rs`, under `default_latency_test`:

```rust
    /// 默认的延迟测试连接
    pub default_latency_test: String,

    /// Latency test timeout in milliseconds.
    #[serde(default = "default_latency_timeout_ms")]
    pub default_latency_timeout_ms: u64,
```

Beside `default_max_log_file_size`:

```rust
/// The latency test timeout a fresh or upgraded install starts with.
pub const DEFAULT_LATENCY_TIMEOUT_MS: u64 = 5000;

fn default_latency_timeout_ms() -> u64 {
    DEFAULT_LATENCY_TIMEOUT_MS
}

/// A latency test waits between one second and one minute.
pub fn validate_latency_timeout(timeout_ms: u64) -> Result<(), &'static str> {
    if (1000..=30000).contains(&timeout_ms) {
        Ok(())
    } else {
        Err("the latency test timeout must be between 1 and 30 seconds")
    }
}
```

In `Default for NyanpasuAppConfig` add `default_latency_timeout_ms: default_latency_timeout_ms(),` after `default_latency_test`. In `mod patch_tests` add:

```rust
    #[test]
    fn latency_timeout_defaults_when_missing_and_is_bounded() {
        let mut value = serde_json::to_value(NyanpasuAppConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("default_latency_timeout_ms");
        let config: NyanpasuAppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.default_latency_timeout_ms, DEFAULT_LATENCY_TIMEOUT_MS);

        assert!(validate_latency_timeout(1000).is_ok());
        assert!(validate_latency_timeout(30000).is_ok());
        assert!(validate_latency_timeout(999).is_err());
        assert!(validate_latency_timeout(30001).is_err());
    }
```

(If `serde_json` is not a dev-dependency of nyanpasu-config, use the serializer the existing `patch_tests` use.)

Run: `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-config latency_timeout` → PASS.

- [ ] **Step 2: Reject an out-of-range timeout on save**

`backend/tauri/src/state/config_error.rs`, after `InvalidCoreLogs`:

```rust
    #[snafu(display("invalid latency test timeout: {reason}"))]
    InvalidLatencyTimeout { reason: String },
```

`backend/tauri/src/state/application.rs` `commit`, after the `validate_update_sources` block:

```rust
        nyanpasu_config::application::validate_latency_timeout(next.default_latency_timeout_ms)
            .map_err(|reason| ConfigError::InvalidLatencyTimeout {
                reason: reason.into(),
            })?;
```

If an exhaustive `match` on `ConfigError` elsewhere fails to compile, add the arm beside `InvalidCoreLogs`, mapping it the same way. Add a test next to the existing `InvalidUpdateSources` tests in `backend/tauri/src/client/application.rs` (~line 288) that patches `default_latency_timeout_ms: Some(500)` and asserts `Err(ConfigError::InvalidLatencyTimeout { .. })`, following the shape of those tests.

In `impact.rs` `app_cases()`, after the `default_latency_test` case:

```rust
            AppCase {
                field: "default_latency_timeout_ms",
                mutate: |app| app.default_latency_timeout_ms = 10000,
                impact: RuntimeImpact::None,
                owners: &[],
            },
```

- [ ] **Step 3: `ProxyGroup` carries the test URL and expected status**

In `core/clash/proxies.rs` `ProxyGroup` add after `fixed`:

```rust
    /// The URL the core tests this group's members with; `None` when unset.
    pub test_url: Option<String>,
    /// The status codes a test must return, in the core's range syntax.
    pub expected_status: Option<String>,
```

and in `from_record`:

```rust
            test_url: record.test_url.clone().filter(|url| !url.is_empty()),
            expected_status: record
                .expected_status
                .clone()
                .filter(|status| !status.is_empty()),
```

Test in the same file's `mod tests`:

```rust
    #[test]
    fn a_group_carries_its_test_url_and_expected_status() {
        let mut group = item("g", "URLTest", Some(vec!["DIRECT"]), Some("DIRECT"));
        group.test_url = Some("https://cp.cloudflare.com".into());
        group.expected_status = Some("204".into());
        let mut empty = item("e", "Selector", Some(vec!["DIRECT"]), Some("DIRECT"));
        empty.test_url = Some(String::new());
        empty.expected_status = Some(String::new());
        let proxies =
            Proxies::from_responses(records(&[group, empty]), &IndexMap::new(), None).unwrap();
        let by_name = |name: &str| proxies.groups.iter().find(|g| g.name.as_str() == name).unwrap();
        assert_eq!(by_name("g").test_url.as_deref(), Some("https://cp.cloudflare.com"));
        assert_eq!(by_name("g").expected_status.as_deref(), Some("204"));
        assert_eq!(by_name("e").test_url, None);
        assert_eq!(by_name("e").expected_status, None);
    }
```

Fix any `ProxyGroup { .. }` literals the compiler flags (tray tests build `TrayGroup`, not `ProxyGroup`; check `grep -rn "ProxyGroup {" backend/tauri/src`).

- [ ] **Step 4: `delay_query` reads the app config (test first)**

Replace `delay_query` in `client/clash_api.rs`:

```rust
/// Used only when the app config's default test URL is empty too.
const FALLBACK_LATENCY_TEST_URL: &str = "http://www.gstatic.com/generate_204";

/// A test's URL is the caller's, else the app's default; its timeout is
/// always the app's.
fn delay_query(
    url: Option<String>,
    expected: Option<String>,
    app: &NyanpasuAppConfig,
) -> Result<DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .or_else(|| Some(app.default_latency_test.clone()).filter(|url| !url.is_empty()))
        .unwrap_or_else(|| FALLBACK_LATENCY_TEST_URL.into());
    let query = DelayQuery::new(
        url.parse()?,
        Duration::from_millis(app.default_latency_timeout_ms),
    )?;
    Ok(match expected.filter(|expected| !expected.is_empty()) {
        Some(expected) => query.with_expected(ExpectedStatus::new(expected)?),
        None => query,
    })
}
```

(import `clash_api::ExpectedStatus` and `nyanpasu_config::application::NyanpasuAppConfig`), and the callers:

```rust
    pub async fn proxy_delay(
        &self,
        name: String,
        provider: Option<String>,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<Delay> {
        let query = delay_query(url, expected, &self.get_app_config().await?)?;
        // … unchanged
    }

    pub async fn group_delay(
        &self,
        group: String,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<IndexMap<ProxyName, u16>> {
        let query = delay_query(url, expected, &self.get_app_config().await?)?;
        // … unchanged
    }
```

Tests in the existing `mod tests` of `clash_api.rs`:

```rust
    #[test]
    fn a_delay_query_falls_back_to_the_app_default_then_gstatic() {
        use nyanpasu_config::application::NyanpasuAppConfig;
        let mut app = NyanpasuAppConfig::default();
        app.default_latency_test = "https://cp.cloudflare.com/generate_204".into();
        app.default_latency_timeout_ms = 3000;

        let query = super::delay_query(Some("https://example.com/".into()), None, &app).unwrap();
        assert_eq!(query.url.as_str(), "https://example.com/");
        assert_eq!(query.timeout, std::time::Duration::from_millis(3000));
        assert!(query.expected.is_none());

        let query = super::delay_query(Some(String::new()), None, &app).unwrap();
        assert_eq!(query.url.as_str(), "https://cp.cloudflare.com/generate_204");

        app.default_latency_test.clear();
        let query = super::delay_query(None, None, &app).unwrap();
        assert_eq!(query.url.as_str(), "http://www.gstatic.com/generate_204");
    }

    #[test]
    fn a_delay_query_validates_the_expected_status() {
        let app = nyanpasu_config::application::NyanpasuAppConfig::default();
        let query = super::delay_query(None, Some("200-299".into()), &app).unwrap();
        assert_eq!(query.expected.unwrap().as_str(), "200-299");
        assert!(super::delay_query(None, Some(String::new()), &app).unwrap().expected.is_none());
        assert!(super::delay_query(None, Some("not a status".into()), &app).is_err());
    }
```

(If `"not a status"` happens to parse, pick a value `validate_expected_status` rejects — read it in `clash-api/src/api/proxies.rs`.)

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib client::clash_api` → PASS.

- [ ] **Step 5: RPC parameters and bindings**

In `ipc.rs`:

```rust
pub async fn clash_api_get_proxy_delay(
    client: State<'_, NyanpasuClient>,
    name: String,
    provider: Option<String>,
    url: Option<String>,
    expected: Option<String>,
) -> Result<clash_api::Delay> {
    Ok(client.proxy_delay(name, provider, url, expected).await?)
}
```

```rust
pub async fn clash_api_get_group_delay(
    client: State<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}
```

Regenerate bindings (command in Global Constraints). Then make the frontend compile without changing behavior:

- `use-clash-proxies.ts`: every `clashApiGetProxyDelay(name, provider, url)` call gains a trailing `null`; `clashApiGetGroupDelay(group, url)` gains `null`.
- every TS test fixture that builds a `ProxyGroup` literal gains `testUrl: null, expectedStatus: null` (find with `grep -rln "capabilities: {" frontend/*/tests frontend/nyanpasu/perf`).

Run: `pnpm typecheck` → PASS; `cargo test … --lib specta_export` → PASS; `cargo test … --lib core::clash::proxies` → PASS; `pnpm exec vitest run frontend/query/tests/proxy-delay-history.browser.test.tsx` → PASS (the existing call assertions may need the extra `expected: null` argument in `toHaveBeenCalledWith`).

- [ ] **Step 6: Commit**

```bash
git add backend/nyanpasu-config/src/application/mod.rs backend/tauri/src/state/config_error.rs \
  backend/tauri/src/state/application.rs backend/tauri/src/client/application.rs \
  backend/tauri/src/client/application_workflow/impact.rs backend/tauri/src/core/clash/proxies.rs \
  backend/tauri/src/client/clash_api.rs backend/tauri/src/ipc.rs \
  frontend/rpc/src/rpc-bindings.ts frontend/rpc/src/tauri-bindings.ts frontend/query/src/query-bindings.ts \
  frontend/query/src/ipc/use-clash-proxies.ts <each fixture file touched>
git commit -m "feat(proxies): test latency against the configured URL and timeout" \
  -m "Groups now carry the core's testUrl and expectedStatus, delay RPCs accept an expected status,
and a missing URL falls back to default_latency_test instead of a hard-coded gstatic.
The timeout comes from the new default_latency_timeout_ms setting (5 s, 1-30 s)."
```

### Task 2: Shared delay functions in `@nyanpasu/query`

**Files:**

- Create: `frontend/query/src/ipc/proxy-delay.ts`
- Modify: `frontend/query/src/ipc/index.ts` (`export * from './proxy-delay'` after `use-clash-proxies`)
- Create: `frontend/query/tests/proxy-delay.test.ts`
- Modify: `frontend/nyanpasu/src/components/proxies/group-summary.tsx`
- Delete: `frontend/nyanpasu/src/components/proxies/group-delay.ts`, `frontend/nyanpasu/tests/proxy-group-delay.test.ts` (its cases move to the new test)

**Interfaces:**

- Consumes: Task 1 `ProxyGroup.testUrl`.
- Produces (exact):

```ts
export type TestUrlSource = { testUrl?: string | null }
export function groupTestUrl(
  group: TestUrlSource | undefined,
  defaultUrl: string,
): string
export function nodeDelayHistory(
  node: Proxy_Serialize,
  url: string,
): DelayHistory[]
export function latestDelay(
  node: Proxy_Serialize | undefined,
  url: string,
): number | undefined
export type Chain = { path: string[]; leaf?: Proxy_Serialize; parent: string }
export function resolveChain(start: string, proxies: Proxies_Serialize): Chain
export function memberDelay(
  member: string,
  group: TestUrlSource,
  proxies: Proxies_Serialize,
  defaultUrl: string,
): number | undefined
```

- [ ] **Step 1: Write the failing tests**

`frontend/query/tests/proxy-delay.test.ts` — build fixtures like `frontend/nyanpasu/tests/proxy-group-delay.test.ts` (copy its `node`/`groupRecord`/`group`/`snapshot` helpers, adding `testUrl: null, expectedStatus: null` to `group`). Cases:

```ts
test('a group tests against its own URL, else the default', () => {
  expect(groupTestUrl({ testUrl: 'https://a/' }, 'https://d/')).toBe(
    'https://a/',
  )
  expect(groupTestUrl({ testUrl: '' }, 'https://d/')).toBe('https://d/')
  expect(groupTestUrl({ testUrl: null }, 'https://d/')).toBe('https://d/')
  expect(groupTestUrl(undefined, 'https://d/')).toBe('https://d/')
})

test('delays come from extra[url], else history', () => {
  const base = node('n', [10, 20])
  const withExtra = {
    ...base,
    extra: {
      'https://a/': { alive: true, history: [{ time: '', delay: 99 }] },
    },
  }
  expect(latestDelay(withExtra, 'https://a/')).toBe(99)
  expect(latestDelay(withExtra, 'https://b/')).toBe(20) // URL never tested here
  expect(latestDelay({ ...base, extra: {} }, 'https://a/')).toBe(20) // Clash-rs
  expect(latestDelay(base, 'https://a/')).toBe(20) // Meow: no extra
  expect(latestDelay(node('empty'), 'https://a/')).toBeUndefined()
  expect(latestDelay(undefined, 'https://a/')).toBeUndefined()
  expect(nodeDelayHistory(withExtra, 'https://a/')).toEqual([
    { time: '', delay: 99 },
  ])
})

test('a chain follows each nested selection to its leaf', () => {
  const proxies = snapshot(null, [], {
    outer: groupRecord('outer', 'inner', ['inner']),
    inner: { ...groupRecord('inner', 'leaf', ['leaf']), testUrl: 'https://i/' },
    leaf: node('leaf', [30]),
  })
  expect(resolveChain('outer', proxies)).toEqual({
    path: ['inner', 'leaf'],
    leaf: proxies.nodes.leaf,
    parent: 'inner',
  })
})

test('a chain stops at a cycle or a missing node', () => {
  const cycle = snapshot(null, [], {
    a: groupRecord('a', 'b', ['b']),
    b: groupRecord('b', 'a', ['a']),
  })
  expect(resolveChain('a', cycle).leaf).toBeUndefined()
  const missing = snapshot(null, [], { a: groupRecord('a', 'gone', ['gone']) })
  expect(resolveChain('a', missing).leaf).toBeUndefined()
  const unselected = snapshot(null, [], { a: groupRecord('a', null, []) })
  expect(resolveChain('a', unselected)).toEqual({ path: [], parent: 'a' })
})

test("a member that is a group reports its leaf under the leaf's group URL", () => {
  const inner = {
    ...groupRecord('inner', 'leaf', ['leaf']),
    testUrl: 'https://i/',
  }
  const leaf = {
    ...node('leaf', [70]),
    extra: {
      'https://i/': { alive: true, history: [{ time: '', delay: 40 }] },
    },
  }
  const proxies = snapshot(null, [], { inner, leaf, plain: node('plain', [5]) })
  const outer = { testUrl: 'https://o/' }
  expect(memberDelay('inner', outer, proxies, 'https://d/')).toBe(40)
  expect(memberDelay('plain', outer, proxies, 'https://d/')).toBe(5)
  expect(memberDelay('gone', outer, proxies, 'https://d/')).toBeUndefined()
})
```

Run: `pnpm exec vitest run frontend/query/tests/proxy-delay.test.ts` → FAIL (module missing).

- [ ] **Step 2: Implement `proxy-delay.ts`**

```ts
import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
} from '@nyanpasu/rpc/types'

/** Anything that names the URL its members are tested with. */
export type TestUrlSource = { testUrl?: string | null }

/** The URL a group's members are tested with: its own, else the default. */
export function groupTestUrl(
  group: TestUrlSource | undefined,
  defaultUrl: string,
): string {
  return group?.testUrl || defaultUrl
}

/**
 * A node's samples for one URL. Mihomo keeps them per URL in `extra`;
 * Clash-rs (`extra: {}`) and Meow (no `extra`) only have `history`.
 */
export function nodeDelayHistory(
  node: Proxy_Serialize,
  url: string,
): DelayHistory[] {
  return node.extra?.[url]?.history ?? node.history
}

/** The latest sample's delay; `0` is a failed test, `undefined` untested. */
export function latestDelay(
  node: Proxy_Serialize | undefined,
  url: string,
): number | undefined {
  return node ? nodeDelayHistory(node, url).at(-1)?.delay : undefined
}

export type Chain = {
  /** Every name after `start`, down to and including the leaf. */
  path: string[]
  /** Absent when the chain is unselected, cycles, or names a missing node. */
  leaf?: Proxy_Serialize
  /** The group that directly holds the last name in `path`. */
  parent: string
}

/** Follows `start`'s selection through nested groups to a leaf node. */
export function resolveChain(start: string, proxies: Proxies_Serialize): Chain {
  const visited = new Set([start])
  const path: string[] = []
  let parent = start
  let name = proxies.nodes[start]?.now

  while (name && !visited.has(name)) {
    const node: Proxy_Serialize | undefined = proxies.nodes[name]
    path.push(name)
    if (!node) {
      return { path, parent }
    }
    if (!node.all) {
      return { path, leaf: node, parent }
    }
    visited.add(name)
    parent = name
    name = node.now
  }

  return { path, parent }
}

/**
 * A member's delay as its group sees it: a nested group reports its leaf,
 * tested against the URL of the group that directly holds that leaf.
 */
export function memberDelay(
  member: string,
  group: TestUrlSource,
  proxies: Proxies_Serialize,
  defaultUrl: string,
): number | undefined {
  const node = proxies.nodes[member]
  if (!node?.all) {
    return latestDelay(node, groupTestUrl(group, defaultUrl))
  }

  const chain = resolveChain(member, proxies)
  return latestDelay(
    chain.leaf,
    groupTestUrl(proxies.nodes[chain.parent], defaultUrl),
  )
}
```

Note `resolveChain` with a missing node: `path` includes the missing name (useful for display), `leaf` undefined — the missing test expects only `leaf` undefined. The unselected case returns `{ path: [], parent: 'a' }` (no `leaf` key — use `toEqual`, which ignores `undefined` keys).

Run the test → PASS.

- [ ] **Step 3: Sidebar summary uses the shared functions**

`group-summary.tsx`: replace `getGroupSelectedDelay` with

```tsx
const { value: defaultUrl } = useSetting('default_latency_test')

const delay = group.now
  ? memberDelay(group.now, group, proxies, defaultUrl ?? '')
  : undefined
```

(imports from `@nyanpasu/query`). Delete `group-delay.ts` and `frontend/nyanpasu/tests/proxy-group-delay.test.ts`; make sure every case that file covered (chains, cycles, missing nodes, empty history) exists in `proxy-delay.test.ts` (add any missing one).

Run: `pnpm typecheck`, `pnpm exec vitest run frontend/query/tests/proxy-delay.test.ts` → PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/query/src/ipc/proxy-delay.ts frontend/query/src/ipc/index.ts \
  frontend/query/tests/proxy-delay.test.ts frontend/nyanpasu/src/components/proxies/group-summary.tsx \
  frontend/nyanpasu/src/components/proxies/group-delay.ts frontend/nyanpasu/tests/proxy-group-delay.test.ts
git commit -m "feat(proxies): read delays per test URL through shared helpers" \
  -m "groupTestUrl, latestDelay, resolveChain and memberDelay replace the sidebar's own chain walk,
reading extra[url] first and falling back to history on cores without it."
```

### Task 3: Tests and the group page use the group's test URL

**Files:**

- Modify: `frontend/query/src/ipc/use-clash-proxies.ts`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/delay-test-button.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx`
- Modify: `frontend/query/tests/proxy-delay-history.browser.test.tsx`, `frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`

**Interfaces:**

- Consumes: Task 1 RPC args; Task 2 `groupTestUrl`, `nodeDelayHistory`, `latestDelay`.
- Produces: `ClashDelayOptions = { url?: string; expected?: string | null }` (drop the unused `timeout`); `updateProxiesDelay.mutateAsync([name, provider, options?])` and `updateGroupDelay.mutateAsync([group, options?])` pass `options.url ?? null` and `options.expected ?? null`. `ProxyNodeButton` gains a `testUrl: string` prop.

- [ ] **Step 1: Write the failing tests**

In `proxy-delay-history.browser.test.tsx` add:

1. `updateGroupDelay` with `['group', { url: 'https://g/', expected: '204' }]` on an unpinned group calls `clash_api_get_group_delay` with `{ group: 'group', url: 'https://g/', expected: '204' }`.
2. The pinned per-member path passes the same `url` and `expected` to every `clash_api_get_proxy_delay` call.
3. After a test with URL `https://g/`, the cached node's `extra['https://g/'].history` ends with the new sample and `history` ends with it too; a node without `extra` gains `extra: { 'https://g/': { alive: true, history: [sample] } }` (alive is `delay > 0`).
4. `updateProxiesDelay` with `['tested', null, { url: 'https://g/', expected: null }]` calls `clash_api_get_proxy_delay` with `{ name: 'tested', provider: null, url: 'https://g/', expected: null }` and appends to both places.

In `proxy-node-button.browser.test.tsx` add: a node whose `history` ends with 10 and whose `extra['https://g/'].history` ends with 77, rendered with `testUrl="https://g/"`, shows `77` in the delay chip.

Run both files → FAIL.

- [ ] **Step 2: Implement**

`use-clash-proxies.ts`:

```ts
export type ClashDelayOptions = {
  url?: string
  expected?: string | null
}

// Append a delay sample where it is read back: `history` and the tested
// URL's `extra` entry.
const withDelaySample = (
  node: ClashProxiesQueryProxyItem,
  delay: number,
  url: string | null,
): ClashProxiesQueryProxyItem => {
  const sample = {
    time: new Date().toISOString(),
    delay,
  } satisfies DelayHistory

  if (!url) {
    return { ...node, history: [...node.history, sample] }
  }

  const entry = node.extra?.[url]

  return {
    ...node,
    history: [...node.history, sample],
    extra: {
      ...node.extra,
      [url]: {
        alive: delay > 0,
        history: [...(entry?.history ?? []), sample],
      },
    },
  }
}
```

Pass `options?.url ?? null` and `options?.expected ?? null` to `clashApiGetProxyDelay(name, provider, url, expected)` / `clashApiGetGroupDelay(group, url, expected)` (including the pinned per-member path); return `url` from `updateProxiesDelay.mutationFn` alongside `{ name, delay }` and from `updateGroupDelay` via the mutation variables (`onSuccess: (data, [, options]) => …`) so `onSuccess` calls `withDelaySample(node, delay, options?.url ?? null)`.

Group page `$name.tsx`:

```tsx
const { value: defaultUrl } = useSetting('default_latency_test')

const testUrl = groupTestUrl(currentGroup, defaultUrl ?? '')

const delayOptions = useMemo(
  () => ({ url: testUrl, expected: currentGroup?.expectedStatus ?? null }),
  [testUrl, currentGroup?.expectedStatus],
)
```

`handleDelayTest` calls `mutateProxyDelay([proxy.name, proxy.provider, delayOptions])`; pass `testUrl={testUrl}` to `ProxyNodeButton`. `DelayTestButton` gains a `delayOptions: ClashDelayOptions` prop and calls `updateGroupDelay.mutateAsync([name, delayOptions])`; the page passes it.

`proxy-node-button.tsx`: add `testUrl: string` prop; `const history = useMemo(() => nodeDelayHistory(proxy, testUrl), [proxy, testUrl])`; `currentDelay` reads `history.at(-1)?.delay ?? -1`; `<DelayHistory history={history}>`.

Run the two test files → PASS; `pnpm typecheck` → PASS.

- [ ] **Step 3: Commit**

```bash
git add frontend/query/src/ipc/use-clash-proxies.ts \
  "frontend/nyanpasu/src/pages/(main)/main/proxies/group/\$name.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/delay-test-button.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx" \
  frontend/query/tests/proxy-delay-history.browser.test.tsx frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx
git commit -m "feat(proxies): test and show delays for each group's own URL" \
  -m "Latency tests on the group page pass the group's effective URL and expected status, and
samples land in both history and extra[url] so the cards read what was just measured."
```

### Task 4: Latency settings in the Clash settings page (UI)

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/settings/clash/_modules/latency-test-config.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/settings/clash/route.tsx`
- Modify: `frontend/nyanpasu/messages/{en,ko,ru,zh-cn,zh-tw}.json`
- Create: `frontend/nyanpasu/tests/latency-test-config.browser.test.tsx`

**Interfaces:** Consumes `useSetting('default_latency_test')`, `useSetting('default_latency_timeout_ms')`.

Messages (after `settings_clash_settings_field_filter_label`):

| key                                           | en                                            | zh-cn                         |
| --------------------------------------------- | --------------------------------------------- | ----------------------------- |
| `settings_clash_latency_test_label`           | Latency Test                                  | 延迟测试                      |
| `settings_clash_latency_test_url_label`       | Default test URL                              | 默认测速 URL                  |
| `settings_clash_latency_test_url_description` | Used when a group sets no test URL of its own | 组未设置自己的测速 URL 时使用 |
| `settings_clash_latency_test_timeout_label`   | Timeout (seconds)                             | 超时（秒）                    |
| `settings_clash_latency_test_invalid_url`     | Enter an http or https URL                    | 请输入 http 或 https 地址     |
| `settings_clash_latency_test_invalid_timeout` | Enter 1 to 30 seconds                         | 请输入 1 到 30 秒             |

- [ ] **Step 1: Write the failing browser test**

Mock `@nyanpasu/query` the way `traffic-retention-selector.browser.test.tsx` does, with a `useSetting(key)` that keeps per-key state and records `upsert` calls. Cases:

1. Renders the current URL and `5` seconds for `default_latency_timeout_ms: 5000`.
2. Typing `https://cp.cloudflare.com/generate_204` and pressing Apply upserts `default_latency_test` with that value.
3. Setting the timeout to `8` upserts `default_latency_timeout_ms` with `8000`.
4. Timeout `0` or `31`, or URL `ftp://x`, `https://[` or `https://example.com:99999`, shows the matching invalid message and does not upsert.
5. Reset restores the saved values.
6. An empty URL saves the default `http://www.gstatic.com/generate_204`.

Run → FAIL.

- [ ] **Step 2: Implement**

Follow `settings/system/_modules/proxy-guard-config.tsx` exactly (react-hook-form + zod + `Controller`, `Input`/`NumericInput` with `variant="outlined"`, `SettingsCardAnimatedItem` errors and the Reset/Apply row shown only when dirty). One form with two fields; schema:

```ts
const formSchema = z.object({
  url: z
    .string()
    .trim()
    .refine((value) => /^https?:\/\/\S+$/i.test(value), {
      message: m.settings_clash_latency_test_invalid_url(),
    }),
  timeout: z
    .number()
    .min(1, m.settings_clash_latency_test_invalid_timeout())
    .max(30, m.settings_clash_latency_test_invalid_timeout()),
})
```

Submit upserts only the fields that changed (`timeout * 1000` for the setting). Errors from `upsert` use `message(formatError(error), { title: 'Error', kind: 'error', error })`. Root `data-slot="latency-test-config"`. In `route.tsx` add a `LatencyTestSettings` section (`data-slot="latency-test-settings-container"`, `SettingsLabel` = `settings_clash_latency_test_label`, one `SettingsCard`/`SettingsCardContent` wrapping the form, with the URL description under the URL field) rendered inside the deferred block after `<FieldFilterSettings />`.

Run `pnpm web:build` (regenerates Paraglide), then the test → PASS; `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier` → PASS.

- [ ] **Step 3: Commit**

```bash
git add "frontend/nyanpasu/src/pages/(main)/main/settings/clash/_modules/latency-test-config.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/settings/clash/route.tsx" \
  frontend/nyanpasu/messages/en.json frontend/nyanpasu/messages/ko.json frontend/nyanpasu/messages/ru.json \
  frontend/nyanpasu/messages/zh-cn.json frontend/nyanpasu/messages/zh-tw.json \
  frontend/nyanpasu/tests/latency-test-config.browser.test.tsx
git commit -m "feat(settings): configure the default latency test URL and timeout"
```

---

## Phase C2 — `feat/proxies-node-list-tools`

Create the branch from the C1 head after C1's review.

### Task 5: `SearchField` and `FilterChip` in `@nyanpasu/ui` (UI)

**Files:**

- Create: `frontend/ui/src/search-field.tsx`, `frontend/ui/src/chip.tsx`
- Modify: `frontend/ui/src/index.ts` (`export * from './search-field'`, `export * from './chip'`, alphabetical position)
- Create: `frontend/ui/tests/search-field.browser.test.tsx`, `frontend/ui/tests/chip.browser.test.tsx`

**Interfaces (exact):**

```ts
export type SearchFieldProps = Omit<
  ComponentProps<'input'>,
  'value' | 'onChange' | 'type'
> & {
  value: string
  onValueChange: (value: string) => void
  /** Accessible name of the clear button. */
  clearLabel: string
}
export const SearchField: (props: SearchFieldProps) => JSX.Element

export type FilterChipProps = Omit<
  ComponentProps<typeof TogglePrimitive.Root>,
  'children'
> & {
  children: ReactNode
}
export const FilterChip: (props: FilterChipProps) => JSX.Element
```

- [ ] **Step 1: Write the failing tests**

`search-field.browser.test.tsx` (mount with a `useState` harness like `number-stepper.browser.test.tsx`):

1. Renders an `input` of role `searchbox` (`type="search"`) with the placeholder; typing calls `onValueChange` per keystroke.
2. The clear button (`getByRole('button', { name: clearLabel })`) is absent while empty and present with a value; activating it with the keyboard (`Tab` to it, `Enter`) clears the value and moves focus back to the input.
3. `Escape` inside a non-empty input clears it.
4. Root has `data-slot="search-field"`; input `data-slot="search-field-input"`; clear button `data-slot="search-field-clear"`.

`chip.browser.test.tsx`:

1. A `FilterChip` is a `button` with `aria-pressed="false"`; clicking toggles `onPressedChange(true)` and `aria-pressed="true"`; `Space` toggles back.
2. The check icon (`data-slot="filter-chip-check"`) renders only while pressed.
3. `disabled` prevents toggling.

Run: `pnpm exec vitest run frontend/ui/tests/search-field.browser.test.tsx frontend/ui/tests/chip.browser.test.tsx` → FAIL.

- [ ] **Step 2: Implement (Material You, existing tokens)**

`SearchField` — the compact MD3 search bar, visually aligned with the connections/rules search inputs:

- root `div` (`data-slot="search-field"`): `relative flex h-10 min-w-0 items-center rounded-full bg-surface-variant dark:bg-surface-variant/30 text-on-surface`, focus-within ring `focus-within:outline-2 focus-within:outline-primary` (or the project's equivalent focus style; check `button.tsx`/`input.tsx`);
- leading `SearchRounded` icon (`~icons/material-symbols/search-rounded`, `size-5`, `text-on-surface-variant`, `ml-3`, `aria-hidden`);
- `input type="search"` (`data-slot="search-field-input"`): `h-full min-w-0 flex-1 bg-transparent px-3 text-sm outline-none placeholder:text-on-surface-variant`, hide the native cancel button (`[&::-webkit-search-cancel-button]:appearance-none`), `autoComplete/autoCorrect/spellCheck` off like `Input`; `onKeyDown` Escape with a value → `onValueChange('')`;
- trailing clear `button type="button"` (`data-slot="search-field-clear"`, `aria-label={clearLabel}`), shown only when `value` is non-empty: `mr-1 grid size-8 place-content-center rounded-full hover:bg-on-surface/8 focus-visible:outline-2 focus-visible:outline-primary`, `CloseRounded` icon `size-5`; on click `onValueChange('')` then focus the input (keep a ref).

`FilterChip` — MD3 filter chip on `Toggle` from `radix-ui` (`import { Toggle as TogglePrimitive } from 'radix-ui'`):

- `data-slot="filter-chip"`, `h-8 inline-flex items-center gap-2 rounded-lg px-4 text-sm font-medium select-none cursor-pointer transition-colors`;
- unpressed: `border border-outline-variant text-on-surface-variant hover:bg-on-surface-variant/8`;
- pressed (`data-[state=on]:`): `border-transparent bg-secondary-container text-on-secondary-container pl-2` with a leading `CheckRounded` (`size-[18px]`, `data-slot="filter-chip-check"`) that animates in (`motion` `AnimatePresence`, width/opacity, ≤ 200 ms; respect `useReducedMotion` if the project uses it elsewhere);
- focus-visible ring as above; `disabled` → `opacity-38 cursor-not-allowed`.

Do not touch `frontend/nyanpasu/src/pages/(main)/_modules/filter-chip.tsx` (a removable input chip, different component).

Run tests → PASS; `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier` → PASS.

- [ ] **Step 3: Commit**

```bash
git add frontend/ui/src/search-field.tsx frontend/ui/src/chip.tsx frontend/ui/src/index.ts \
  frontend/ui/tests/search-field.browser.test.tsx frontend/ui/tests/chip.browser.test.tsx
git commit -m "feat(ui): add a Material You search field and filter chip"
```

### Task 6: Node list search, sort and filter functions

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/node-list.ts`
- Create: `frontend/nyanpasu/tests/proxy-node-list.test.ts`

**Interfaces (exact):**

```ts
export const NODE_SORTS = ['default', 'name', 'delay'] as const
export type NodeSort = (typeof NODE_SORTS)[number]
export type NodeView = { sort: NodeSort; hideUnavailable: boolean }
export const DEFAULT_NODE_VIEW: NodeView = {
  sort: 'default',
  hideUnavailable: false,
}
export const NODE_VIEW_KV_KEY = 'proxies-node-view'
/** Coerces whatever the KV store holds into a NodeView. */
export function toNodeView(value: unknown): NodeView
export function matchesNodeSearch(node: Proxy_Serialize, query: string): boolean
/**
 * The members to show, in order. `delayOf` returns memberDelay for a name.
 */
export function visibleMembers(input: {
  group: ProxyGroup
  proxies: Proxies_Serialize
  query: string
  view: NodeView
  delayOf: (name: string) => number | undefined
}): string[]
```

- [ ] **Step 1: Write the failing tests** (`proxy-node-list.test.ts`, fixtures as in Task 2):

```ts
test('every space-separated term must match the name or type, ignoring case', () => {
  const n = { ...node('HK 香港-01'), type: 'Vless' }
  expect(matchesNodeSearch(n, '')).toBe(true)
  expect(matchesNodeSearch(n, '  ')).toBe(true)
  expect(matchesNodeSearch(n, 'hk')).toBe(true)
  expect(matchesNodeSearch(n, '香港 vless')).toBe(true)
  expect(matchesNodeSearch(n, ' HK   01 ')).toBe(true)
  expect(matchesNodeSearch(n, 'hk jp')).toBe(false)
})

test('names sort naturally and delays ascend with untested then failed last', () => {
  // members: a=50, b=undefined, c=0, d=10, 'HK-10', 'HK-2'
  // name → ['a','b','c','d','HK-2','HK-10'] (numeric collation, case-insensitive)
  // delay → ['d','a','b','c'] for members a,b,c,d (ties keep config order)
  // default → config order
})

test('hiding unavailable members keeps untested ones and the current selection', () => {
  // members: ok=20, failed=0, untested=undefined, current(now)=0
  // hideUnavailable → ['ok','untested','current']
})

test('search and filters combine; unknown members are dropped', () => {
  // a member missing from proxies.nodes never appears; query + hide + sort apply together
})

test('a stored view is coerced', () => {
  expect(toNodeView(null)).toEqual(DEFAULT_NODE_VIEW)
  expect(toNodeView({ sort: 'delay', hideUnavailable: true })).toEqual({
    sort: 'delay',
    hideUnavailable: true,
  })
  expect(toNodeView({ sort: 'bogus', hideUnavailable: 'yes' })).toEqual(
    DEFAULT_NODE_VIEW,
  )
})
```

Write the commented cases as real assertions with the stated expectations.

Run → FAIL.

- [ ] **Step 2: Implement**

```ts
const collator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
})

export function matchesNodeSearch(node: Proxy_Serialize, query: string) {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
  const text = `${node.name} ${node.type}`.toLowerCase()
  return terms.every((term) => text.includes(term))
}

// Tested delays first (ascending), then untested, then failed.
const delayRank = (delay: number | undefined) =>
  delay === undefined
    ? Number.MAX_SAFE_INTEGER - 1
    : delay <= 0
      ? Number.MAX_SAFE_INTEGER
      : delay

export function visibleMembers({ group, proxies, query, view, delayOf }) {
  const members = group.all.filter((name) => {
    const node = proxies.nodes[name]
    if (!node || !matchesNodeSearch(node, query)) return false
    if (view.hideUnavailable && name !== group.now && delayOf(name) === 0)
      return false
    return true
  })

  switch (view.sort) {
    case 'name':
      return members.sort(collator.compare)
    case 'delay':
      return members
        .map((name, index) => ({ name, index, rank: delayRank(delayOf(name)) }))
        .sort((a, b) => a.rank - b.rank || a.index - b.index)
        .map(({ name }) => name)
    default:
      return members
  }
}
```

(add the types; `toNodeView` validates `sort` against `NODE_SORTS` and `hideUnavailable` as boolean, falling back to `DEFAULT_NODE_VIEW` per field). Run → PASS.

- [ ] **Step 3: Commit**

```bash
git add "frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/node-list.ts" \
  frontend/nyanpasu/tests/proxy-node-list.test.ts
git commit -m "feat(proxies): match, sort and filter a group's members"
```

### Task 7: Group page toolbar (UI)

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/node-list-toolbar.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/route.tsx` (`searchQuery` → `q`)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/_modules/proxies-navigate.tsx` (links keep `q`)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx` (`searchText` prop → `HighlightText`)
- Modify: `frontend/nyanpasu/messages/*.json`
- Create: `frontend/nyanpasu/tests/proxy-node-list-toolbar.browser.test.tsx`
- Regenerate if the router plugin changes it: `frontend/nyanpasu/src/route-tree.gen.ts` (via `pnpm web:build`; commit only if it changed)

**Interfaces:** Consumes Task 5 `SearchField`, `FilterChip`; Task 6 `visibleMembers`, `toNodeView`, `NODE_VIEW_KV_KEY`, `DEFAULT_NODE_VIEW`, `NodeSort`; Task 2 `memberDelay`; `useKvStorage`, `useSearchTerm` (`pages/(main)/main/_modules/use-search-term.ts`).

Messages (after `proxies_clear_fixed_failed_message`):

| key                               | en                  | zh-cn          |
| --------------------------------- | ------------------- | -------------- |
| `proxies_node_search_placeholder` | Search nodes        | 搜索节点       |
| `proxies_node_search_clear`       | Clear search        | 清除搜索       |
| `proxies_node_sort_label`         | Sort                | 排序           |
| `proxies_node_sort_default`       | Default order       | 默认顺序       |
| `proxies_node_sort_name`          | Name                | 名称           |
| `proxies_node_sort_delay`         | Latency             | 延迟           |
| `proxies_node_hide_unavailable`   | Hide unavailable    | 隐藏不可用     |
| `proxies_node_no_match`           | No matching nodes   | 没有匹配的节点 |
| `proxies_locate_current_node`     | Locate current node | 定位当前节点   |

- [ ] **Step 1: Write the failing browser test**

Render the toolbar in isolation (`node-list-toolbar.tsx` takes props, no router/query): `search`, `onSearchChange`, `view`, `onViewChange`. Cases:

1. Typing in the searchbox calls `onSearchChange`; clearing works.
2. Opening the sort menu (button named `proxies_node_sort_label`) shows three `menuitemradio`s with the current one checked; picking Latency calls `onViewChange({ ...view, sort: 'delay' })`.
3. The hide-unavailable chip reflects `view.hideUnavailable` via `aria-pressed` and toggles it.
4. Toolbar root `data-slot="proxies-node-list-toolbar"`.

Run → FAIL.

- [ ] **Step 2: Implement the toolbar**

`GroupHeader` gains an optional `bottom?: ReactNode` prop so the toolbar sticks together with the header: its root keeps `sticky top-0 z-10 bg-mixed-background` and becomes `flex flex-col`; the current row (back button + children, with all its existing flex, padding and `group-data-[scroll-direction=down]` classes) moves into an inner `div`; `bottom` renders after that row. The page passes `<NodeListToolbar … />` as `bottom`.

`node-list-toolbar.tsx` exports `NodeListToolbar` (root `data-slot="proxies-node-list-toolbar"`, `flex items-center gap-2 px-2 pb-2 md:px-4`), plus two small components used by the page so they are testable without the router: `LocateCurrentNodeButton({ disabled, onLocate })` (the existing `Radar` icon button, with `aria-label`/`Tooltip` `proxies_locate_current_node`) and `NoMatchingNodes()` (`<p data-slot="proxies-node-no-match" className="text-on-surface-variant p-8 text-center text-sm">`). Toolbar contents:

- `SearchField` (`className="flex-1"`, placeholder `proxies_node_search_placeholder`, `clearLabel` `proxies_node_search_clear`);
- sort: `Tooltip` + icon `Button` (`SortRounded`, `aria-label` `proxies_node_sort_label`) as `DropdownMenuTrigger`; `DropdownMenuContent` with `DropdownMenuRadioGroup value={view.sort}` and three `DropdownMenuRadioItem`s;
- `FilterChip pressed={view.hideUnavailable}` labelled `proxies_node_hide_unavailable`.

On narrow containers the chip label may wrap; keep `whitespace-nowrap` on the chip and let the search field shrink (`min-w-0`).

- [ ] **Step 3: Wire the page**

`route.tsx`: `searchSchema` field `searchQuery` → `q: z.string().optional()`.

`proxies-navigate.tsx`: the group `Link` keeps the search term: `search={(previous) => ({ q: previous.q })}`.

`$name.tsx`:

```tsx
const { q } = Route.useSearch()

const navigate = Route.useNavigate()

const writeQuery = useCallback(
  (next: string | undefined) =>
    navigate({
      search: (previous) => ({ ...previous, q: next }),
      replace: true,
    }),
  [navigate],
)

const [search, setSearch] = useSearchTerm(q, writeQuery)

const deferredSearch = useDeferredValue(search)

const [storedView, setView] = useKvStorage<NodeView>(
  NODE_VIEW_KV_KEY,
  DEFAULT_NODE_VIEW,
  { migrate: toNodeView },
)

const members = useMemo(
  () =>
    currentGroup && proxies
      ? visibleMembers({
          group: currentGroup,
          proxies,
          query: deferredSearch,
          view: storedView,
          delayOf: (name) =>
            memberDelay(name, currentGroup, proxies, defaultUrl ?? ''),
        })
      : [],
  [currentGroup, proxies, deferredSearch, storedView, defaultUrl],
)
```

Virtualizer `count: members.length`; items read `members[virtualItem.index]`; `key={name}` instead of the index. The header's `Radar` button is replaced by `<LocateCurrentNodeButton disabled={currentIndex === -1} onLocate={…} />` where `currentIndex = members.indexOf(currentGroup.now)` and `onLocate` scrolls to `currentIndex`. When `members.length === 0` and the group has members, render `<NoMatchingNodes />` instead of the list. Pass `searchText={deferredSearch.trim().includes(' ') ? '' : deferredSearch.trim()}` to `ProxyNodeButton`, which renders the name through `HighlightText` (`@nyanpasu/ui`) — `HighlightText` highlights one substring, so multi-term queries are not highlighted. A `setView` failure shows `message(formatError(error), { kind: 'error', error })`.

Add to the toolbar browser test: `LocateCurrentNodeButton` with `disabled` is disabled and does not call `onLocate`; enabled, a click calls it once; `NoMatchingNodes` renders `proxies_node_no_match`.

Run `pnpm web:build`, tests → PASS; `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier` → PASS. Manually check with the WebKit preview harness or `pnpm dev` if available (screenshot the toolbar in light/dark, wide/narrow); report what was checked.

- [ ] **Step 4: Commit**

```bash
git add <every file above that changed>
git commit -m "feat(proxies): search, sort and filter a group's nodes" \
  -m "A toolbar under the group header searches the current group (q in the URL), sorts by
default order, name or latency, and hides failed nodes. The sort and filter persist in the
backend KV store under proxies-node-view."
```

---

## Phase C3 — `perf/proxies-trim-extra`

### Task 8: Trim `extra` on the RPC output

**Files:**

- Modify: `backend/tauri/src/core/clash/proxies.rs` (`retain_extra`, `trim_for_frontend`, tests)
- Modify: `backend/tauri/src/client/clash_api.rs` (`get_proxies`, `refresh_proxies`)

**Interfaces:**

- Produces: `impl Proxies { pub fn retain_extra(&mut self, keep: &HashSet<&str>); pub fn trim_for_frontend(self, default_url: &str) -> Self }`.

- [ ] **Step 1: Write the failing tests** (in `core/clash/proxies.rs` `mod tests`):

```rust
    fn extra(urls: &[&str]) -> Option<IndexMap<String, clash_api::ProxyExtra>> {
        Some(
            urls.iter()
                .map(|url| {
                    (url.to_string(), clash_api::ProxyExtra { alive: true, history: Vec::new() })
                })
                .collect(),
        )
    }

    #[test]
    fn trimming_keeps_only_group_urls_and_the_default() {
        let mut group = item("g", "URLTest", Some(vec!["a", "DIRECT"]), Some("a"));
        group.test_url = Some("https://g/".into());
        let mut a = item("a", "Vless", None, None);
        a.extra = extra(&["https://g/", "https://d/", "https://other/"]);
        let mut proxies = records(&[group]);
        proxies.insert(name("a"), a);
        let trimmed = Proxies::from_responses(proxies, &IndexMap::new(), None)
            .unwrap()
            .trim_for_frontend("https://d/");
        let kept: Vec<_> = trimmed.nodes[&name("a")].extra.as_ref().unwrap().keys().cloned().collect();
        assert_eq!(kept, ["https://g/", "https://d/"]);
    }

    #[test]
    fn trimming_leaves_nodes_without_extra_alone() {
        let mut a = item("a", "Vless", None, None);
        a.extra = Some(IndexMap::new());
        let mut proxies = records(&[]);
        proxies.insert(name("a"), a);
        let trimmed = Proxies::from_responses(proxies, &IndexMap::new(), None)
            .unwrap()
            .trim_for_frontend("https://d/");
        assert_eq!(trimmed.nodes[&name("a")].extra, Some(IndexMap::new()));
        assert_eq!(trimmed.nodes[&name("DIRECT")].extra, None);
    }

    /// Prints the payload saved for a 1000-node subscription; numbers go in the PR.
    #[test]
    fn trimming_shrinks_a_large_snapshot() {
        let urls = ["https://g/", "https://other-1/", "https://other-2/"];
        let history: Vec<clash_api::DelayHistory> = (0..10)
            .map(|delay| serde_json::from_value(json!({"time":"2026-10-06T12:34:56.123456789+08:00","delay":delay})).unwrap())
            .collect();
        let names: Vec<String> = (0..1000).map(|i| format!("node-{i}")).collect();
        let mut group = item("g", "URLTest", Some(names.iter().map(String::as_str).collect()), None);
        group.test_url = Some(urls[0].into());
        let mut proxies = records(&[group]);
        for node_name in &names {
            let mut node = item(node_name, "Vless", None, None);
            node.history = history.clone();
            node.extra = Some(urls.iter().map(|url| (url.to_string(), clash_api::ProxyExtra { alive: true, history: history.clone() })).collect());
            proxies.insert(name(node_name), node);
        }
        let full = Proxies::from_responses(proxies, &IndexMap::new(), None).unwrap();
        let before = serde_json::to_vec(&full).unwrap().len();
        let after = serde_json::to_vec(&full.trim_for_frontend(urls[0])).unwrap().len();
        println!("proxies payload: {before} -> {after} bytes");
        assert!(after < before);
    }
```

Run `cargo test … --lib core::clash::proxies::tests::trimming -- --nocapture` → FAIL (methods missing).

- [ ] **Step 2: Implement**

```rust
impl Proxies {
    /// Drops every node's per-URL delay records except those in `keep`.
    pub fn retain_extra(&mut self, keep: &HashSet<&str>) {
        for node in self.nodes.values_mut() {
            if let Some(extra) = node.extra.as_mut() {
                extra.retain(|url, _| keep.contains(url.as_str()));
            }
        }
    }

    /// The frontend reads delays only under a group's test URL or the
    /// default one, so other URLs' records are not sent.
    pub fn trim_for_frontend(mut self, default_url: &str) -> Self {
        let urls: Vec<String> = self
            .global
            .iter()
            .chain(&self.groups)
            .filter_map(|group| group.test_url.clone())
            .chain([default_url.to_owned()])
            .collect();
        let keep: HashSet<&str> = urls.iter().map(String::as_str).collect();
        self.retain_extra(&keep);
        self
    }
}
```

In `client/clash_api.rs`:

```rust
    pub async fn get_proxies(&self) -> Result<crate::core::clash::proxies::Proxies> {
        let proxies = self.inner.proxies.get(false).await?;
        Ok(proxies.trim_for_frontend(&self.get_app_config().await?.default_latency_test))
    }
    pub async fn refresh_proxies(&self) -> Result<crate::core::clash::proxies::Proxies> {
        let proxies = self.inner.proxies.get(true).await?;
        Ok(proxies.trim_for_frontend(&self.get_app_config().await?.default_latency_test))
    }
```

Check every caller of `get_proxies`/`refresh_proxies` (`grep -rn "\.get_proxies()\|\.refresh_proxies()" backend/tauri/src`): if a non-RPC caller (tray, dashboard) needs full `extra`, leave it reading `proxies_snapshot()` or the actor; report what you found.

Run the tests with `--nocapture`, note the printed numbers for the PR description; `cargo test … --lib core::` → PASS (known failure aside); `pnpm lint:clippy` → no new warnings.

- [ ] **Step 3: Commit**

```bash
git add backend/tauri/src/core/clash/proxies.rs backend/tauri/src/client/clash_api.rs
git commit -m "perf(proxies): send only the delay records the frontend reads" \
  -m "get_proxies and mutate_proxies keep extra entries for group test URLs and the default
test URL; the actor snapshot the tray reads stays complete."
```

---

## Phase C4 — `feat/proxies-group-header-p2`

### Task 9: Group header availability, chain and nested-member leaf (UI)

**Files:**

- Modify: `frontend/query/src/ipc/proxy-delay.ts` (add `availableCount`), `frontend/query/tests/proxy-delay.test.ts`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx`
- Modify: `frontend/nyanpasu/messages/*.json`
- Modify: `frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`

**Interfaces:** Produces `availableCount(group: ProxyGroup, proxies: Proxies_Serialize, defaultUrl: string): number` — members whose `memberDelay` is `> 0`.

Messages (after the Task 7 keys): `proxies_group_available` — en `Available {available}/{total}`, zh-cn `可用 {available}/{total}`.

- [ ] **Step 1: Failing tests**

`proxy-delay.test.ts`: `availableCount` counts a nested group by its leaf, ignores failed (`0`) and untested members, and returns 0 for an empty group.

`proxy-node-button.browser.test.tsx`: a `ProxyNodeButton` given `leaf="HK-01"` shows `→ HK-01` (`data-slot="proxy-node-leaf"`) and the given `delay` prop; without `leaf` nothing extra renders.

- [ ] **Step 2: Implement**

`ProxyNodeButton`: replace the internal delay derivation with props computed by the page — `delay: number | undefined` (from `memberDelay`) and `leaf?: string` (from `resolveChain(name).leaf?.name` when the member is a group) — keeping `history` (from `nodeDelayHistory`) for the tooltip. In the second row, between the feature chips and the delay button, render `{leaf && <span data-slot="proxy-node-leaf" className="text-on-surface-variant min-w-0 truncate text-xs" title={leaf}>→ {leaf}</span>}`. Card height stays 64 px.

Group header (`$name.tsx`), under the name: one line `data-slot="proxies-group-status"`, `text-on-surface-variant flex min-w-0 items-center gap-1 text-xs`: `m.proxies_group_available({ available, total })`, then, when `resolveChain(group.name, proxies).path` is non-empty, `·` and the path joined with `›` in a truncating span (`title` = full chain). Keep the existing `proxies-group-fixed` line below it.

Run tests → PASS; typecheck/lint → PASS.

- [ ] **Step 3: Commit**

```bash
git add frontend/query/src/ipc/proxy-delay.ts frontend/query/tests/proxy-delay.test.ts \
  "frontend/nyanpasu/src/pages/(main)/main/proxies/group/\$name.tsx" \
  "frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx" \
  frontend/nyanpasu/messages/*.json frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx
git commit -m "feat(proxies): show a group's availability, chain and nested leaves"
```

### Task 10: Three-mode break-on-proxy-change setting (UI)

**Files:**

- Delete: `frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/_modules/break-when-proxy-change-switch.tsx`
- Create: `frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/_modules/break-when-proxy-change-selector.tsx`
- Modify: the settings route that imports the switch (`grep -rn BreakWhenProxyChangeSwitch frontend/nyanpasu/src`)
- Modify: `frontend/query/src/ipc/settings-conversions.ts` (remove `breaksOnProxyChange`, `proxyChangeBreakMode`), `frontend/query/tests/settings-conversions.test.ts` (remove their cases)
- Modify: `frontend/nyanpasu/messages/*.json`
- Create: `frontend/nyanpasu/tests/break-when-proxy-change-selector.browser.test.tsx`

Messages (replace `settings_nyanpasu_enhance_break_when_proxy_change_label`/`_description` in place):

| key                                                             | en                                                                                                                                            | zh-cn                                                                                |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `settings_nyanpasu_enhance_break_when_proxy_change_label`       | Interrupt connections when switching proxy                                                                                                    | 切换代理时中断连接                                                                   |
| `settings_nyanpasu_enhance_break_when_proxy_change_description` | Which connections to close after a node is selected or a pin is cleared. On Meow, "This group" misses connections made through nested groups. | 选择节点或取消固定后关闭哪些连接。在 Meow 内核上，"仅本组"会漏掉经嵌套组建立的连接。 |
| `settings_nyanpasu_enhance_break_when_proxy_change_off`         | Off                                                                                                                                           | 关闭                                                                                 |
| `settings_nyanpasu_enhance_break_when_proxy_change_group`       | This group                                                                                                                                    | 仅本组                                                                               |
| `settings_nyanpasu_enhance_break_when_proxy_change_all`         | All                                                                                                                                           | 全部                                                                                 |

- [ ] **Step 1: Failing browser test** — mock `useClashSetting('break_connection')` (value `{ on_proxy_change: 'all', … }`); the `SegmentedButton` shows three radio-like items with `all` selected; choosing "This group" calls `upsert({ on_proxy_change: 'proxy_group' })`; an `upsert` rejection shows an error via `message` (mock `@/utils/notification`).

- [ ] **Step 2: Implement** — `SettingsCard data-slot="break-when-proxy-change-selector"` with label/description (`ItemLabel`…) and below it a `SegmentedButton value={value.on_proxy_change} onValueChange={(next) => next && handleChange(next as ProxyChangeBreakMode)}` with three `SegmentedButtonItem`s (`off`, `proxy_group`, `all`), following `proxy-port-config.tsx`'s usage. Remove the two conversion functions and their tests; check nothing else imports them.

Run tests, typecheck, lint → PASS.

- [ ] **Step 3: Commit**

```bash
git add <files above>
git commit -m "feat(settings): choose which connections break on a proxy change"
```

### Task 11: Provider health check, backend

**Files:**

- Modify: `backend/tauri/src/core/actor_v2/api.rs` (wrapper)
- Modify: `backend/tauri/src/core/proxies.rs` (message, handler, client method, tests)
- Modify: `backend/tauri/src/client/clash_api.rs` (facade)
- Modify: `backend/tauri/src/ipc.rs`, `backend/tauri/src/specta_export.rs`
- Regenerate bindings

**Interfaces:** Produces `ProxiesClient::healthcheck_provider(name: String) -> Result<()>`, `NyanpasuClient::healthcheck_proxy_provider(name: String) -> Result<()>`, RPC mutation `clash_api_healthcheck_proxy_provider(name)` → `api.mutations.clashApiHealthcheckProxyProvider`.

- [ ] **Step 1: Failing actor tests** — in `core/proxies.rs` tests: add route `.route("/providers/proxies/{name}/healthcheck", get(healthcheck))` where `healthcheck` pushes `"healthcheck"` to `calls`, asserts the name is `PROVIDER`, and returns `StatusCode::ACCEPTED` when a new `Fixture::accepted: AtomicBool` is set, else `NO_CONTENT` (or `SERVICE_UNAVAILABLE` under `fail_mutation`). Tests:
  1. `a_provider_health_check_rereads_the_core`: 204 → `Ok`, calls `["healthcheck", "read"]`; with `accepted` → also `Ok`.
  2. `a_rejected_health_check_rereads_and_reports`: `fail_mutation` → `Err`, snapshot kept, mirrors the existing rejected `update_provider` assertions.

- [ ] **Step 2: Implement** — generalize the provider path rather than duplicating it:

```rust
/// A provider-wide request whose result changes the cached nodes.
enum ProviderAction {
    Update,
    Healthcheck,
}
```

`Message::UpdateProvider { name, reply }` becomes `Message::Provider { name, action: ProviderAction, reply }` (update the shutdown arm). In the handler, call `api.update_proxy_provider(&name.into())` or `api.healthcheck_proxy_provider(&name.into())` by `action`; the rejection/refresh tail is shared; the refresh context message becomes `match action { Update => "provider update succeeded but cache refresh failed", Healthcheck => "provider health check succeeded but cache refresh failed" }` (keep the update text verbatim — an existing test matches it). `ProxiesClient::update_provider` sends `ProviderAction::Update`; new `healthcheck_provider` sends `Healthcheck`. In `actor_v2/api.rs` add beside `update_proxy_provider`:

```rust
    pub async fn healthcheck_proxy_provider(&self, name: &ProviderName) -> Result<(), ApiError> {
        // same shape as update_proxy_provider, calling the crate's healthcheck_proxy_provider
    }
```

(copy `update_proxy_provider`'s body, swapping the call). Facade in `client/clash_api.rs` beside `update_proxy_provider`:

```rust
    pub async fn healthcheck_proxy_provider(&self, name: String) -> Result<()> {
        self.inner.proxies.healthcheck_provider(name).await
    }
```

`ipc.rs` (beside the provider update command):

```rust
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clash_api_healthcheck_proxy_provider(
    client: State<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}
```

Register it in `specta_export.rs` among the mutations, right after the provider update command. Regenerate bindings.

Run: `cargo test … --lib core::proxies` → PASS; `… --lib specta_export` → PASS; `pnpm typecheck` → PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/tauri/src/core/actor_v2/api.rs backend/tauri/src/core/proxies.rs \
  backend/tauri/src/client/clash_api.rs backend/tauri/src/ipc.rs backend/tauri/src/specta_export.rs \
  frontend/rpc/src/rpc-bindings.ts frontend/rpc/src/tauri-bindings.ts frontend/query/src/query-bindings.ts
git commit -m "feat(providers): health-check a proxy provider" \
  -m "The check shares the actor's provider path with updates: a rejection rereads the core and a
success refreshes the cached nodes, whose delays the check has changed."
```

### Task 12: Provider health check button (UI)

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-provider-healthcheck.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/providers/proxies/_modules/info-card.tsx`
- Modify: `frontend/nyanpasu/messages/*.json`
- Create: `frontend/nyanpasu/tests/providers-healthcheck.browser.test.tsx`

Messages (after `providers_update_provider`): `providers_healthcheck_provider` — en `Health Check`, zh-cn `健康检查`; `providers_healthcheck_failed_message` — en `Health check failed for {name}`, zh-cn `{name} 健康检查失败`.

- [ ] **Step 1: Failing test** — mirror `frontend/nyanpasu/tests/providers-refresh.browser.test.tsx` (read it first): clicking the Health Check button invokes `clash_api_healthcheck_proxy_provider` with `{ name }`, then refetches the providers and proxies queries; a rejection shows the failure message.

- [ ] **Step 2: Implement** — the hook mirrors `use-proxies-provider-update.tsx` (block task id `healthcheck-proxies-provider-${name}`), calling `invokeMutation(api.mutations.clashApiHealthcheckProxyProvider, [name])` with `unwrapResult`, then refetching the provider and proxies query keys the update hook refetches. In `InfoCard`'s footer, after the update `Button`, add a `Button` (`data-slot="providers-healthcheck-button"`, `MonitorHeartRounded` icon, `loading` from the block task) labelled `providers_healthcheck_provider`; failure → `message(m.providers_healthcheck_failed_message({ name }), { kind: 'error', error })`.

Run `pnpm web:build`, test → PASS; typecheck/lint → PASS.

- [ ] **Step 3: Commit**

```bash
git add <files above>
git commit -m "feat(providers): add a health check button to proxy providers"
```

---

## Phase checks and delivery

After each phase's last task:

```bash
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
pnpm lint:clippy
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib client::
pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier
pnpm exec vitest run <the phase's new and modified test files>
deno task lint:architecture-ledger
```

Then `/ccg:review` on the phase diff; fold Critical/High fixes into their task commits; re-review until none remain. Push and open a draft PR whose base is the previous phase's branch (C1's base is `feat/proxies-pinned-selection`). The C3 PR description carries the payload numbers from `trimming_shrinks_a_large_snapshot`.

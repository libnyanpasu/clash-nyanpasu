# Proxies API Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve proxies, proxy providers and delay results as clash-api types, derive one proxy-group semantic layer (kind, normalized pin, capabilities) that the tray and the frontend share, and fix the tray's GLOBAL-name and hidden-group defects.

**Architecture:** `Proxies::from_responses` (pure, `backend/tauri/src/core/clash/proxies.rs`) keeps raw `clash_api::Proxy` records in `nodes` and emits app-owned `ProxyGroup` values with `ProxyGroupKind` and `ProxyGroupCapabilities` inferred from field presence. The `ProxiesActor` snapshot, RPC commands and specta bindings carry those types unchanged; the native tray projects `ProxyGroup` into a small `TrayGroup` that is also its diff domain.

**Tech Stack:** Rust 2024 (tauri crate `clash-nyanpasu`, lib `clash_nyanpasu_lib`), clash-api `1.0.0-rc.10` (submodule, read-only here), ractor, specta / unified RPC, React 19, TanStack Query, Vitest (node `unit` + Playwright `browser` projects).

**Spec:** `docs/spec/2026-10-05-proxies-api-unification/design.md`

## Global Constraints

- Follow `AGENTS.md` and `docs/development/*.md`. Rust per `docs/development/rust.md`; TS per `docs/development/typescript.md` (logical blank-line groups, no new UI behavior).
- Branch `refactor/proxies-clash-api-records` in the main checkout. One task = one commit, with the exact subject given in the task. Stage explicit paths only; never stage `.pnpm-store/`, `backend/nyanpasu-runtime`, or `backend/Cargo.lock`.
- Do not modify the clash-api crate (`backend/nyanpasu-runtime/**`).
- Do not hand-edit generated files (`frontend/rpc/src/rpc-bindings.ts`, `frontend/rpc/src/tauri-bindings.ts`, `frontend/query/src/query-bindings.ts`). Regenerate with `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`.
- RPC inputs (`group`, `name`, `provider`, `url` as `String`) and query/mutation classification in `backend/tauri/src/specta_export.rs` stay unchanged.
- No compatibility layer: a task that changes an RPC response migrates every consumer and regenerates bindings in the same commit.
- `Proxies.groups` keeps hidden groups (prior spec `2026-10-05-proxy-group-discovery`); hidden filtering happens only in the tray projection and `use-clash-proxies.ts`.
- Generated TS type names depend on specta's serde phase split. After each regeneration run `grep -nE '^export type (Proxies|Proxy|ProxyGroup|ProxyGroupKind|ProxyGroupCapabilities|DelayHistory|ProxyProvider|SubscriptionInfo|Delay)(_Serialize)? ' frontend/rpc/src/rpc-bindings.ts`. This plan assumes `Proxies_Serialize`, `Proxy_Serialize`, `ProxyGroup`, `DelayHistory`, `ProxyProvider_Serialize`; where the grep shows a different name, use the generated name everywhere the task names that type.
- Rust test command shape: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib <filter>`. Frontend: `pnpm exec vitest run --project unit <file>` or `--project browser <file>`.

## Review Focus

- A proxy provider whose core reports no subscription (`subscriptionInfo: null`) must render on the providers page and widgets without throwing — Task 3 adds `proxies-subscription.browser.test.tsx`.
- A core without a GLOBAL record in Global mode: the tray shows no GLOBAL submenu and `Proxies.global` is `None` — Task 1 and Task 4 tests.
- A URLTest group pinned to one member while `now` temporarily differs (mihomo keeps `fixed`): `ProxyGroup.fixed` and `now` stay distinct — Task 4 `group_fields_are_normalized_from_the_record`.
- A group type this build does not know (fork-only types such as "Weighted"): listed, not selectable, and its `type` string round-trips verbatim — Task 4 `the_group_type_serializes_as_the_core_reports_it` and the capability matrix.
- A hidden GLOBAL in Global mode stays in the tray, matching the page — Task 2 `hidden_groups_stay_out_of_the_tray`.

---

### Task 1: Select the GLOBAL group by its real name (C1)

**Files:**

- Modify: `backend/tauri/src/core/tray/proxies.rs` (`to_tray_proxies`, tests module)

**Interfaces:**

- Consumes: legacy `crate::core::clash::proxies::{Proxies, ProxyGroupItem}` (`global: ProxyGroupItem`, empty `name` when the core has no GLOBAL).
- Produces: `TrayProxies` keyed by the group's real name; Global mode inserts `raw_proxies.global.name` (e.g. `"GLOBAL"`) and copies its `r#type`, and inserts nothing when `global.name` is empty.

- [ ] **Step 1: Write the failing tests**

In the `#[cfg(test)] mod tests` of `backend/tauri/src/core/tray/proxies.rs`, replace the three `global` lines of `global_mode_adds_a_global_entry_rule_mode_does_not` and add a new test after it:

```rust
        let global_mode = to_tray_proxies(Mode::Global, &proxies);
        assert!(global_mode.contains_key("GLOBAL"));
        assert!(!global_mode.contains_key("global"));
        assert!(global_mode.contains_key("GroupA"));
        assert_eq!(global_mode["GLOBAL"].all, vec!["GroupA".to_owned()]);
```

```rust
    /// The core looks a group up by its exact, case-sensitive name, so the
    /// tray must select "GLOBAL", never a lowercase alias; a core without
    /// GLOBAL gets no entry.
    #[test]
    fn global_mode_selects_global_by_its_real_name() {
        let mut proxies = sample_proxies();
        proxies.global.r#type = "Fallback".into();
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GLOBAL", "GroupA"]
        );
        assert_eq!(tray["GLOBAL"].r#type, "Fallback");

        proxies.global = Default::default();
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GroupA"]
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray::proxies`
Expected: FAIL — `global_mode_adds_a_global_entry_rule_mode_does_not` and `global_mode_selects_global_by_its_real_name` find key `"global"`, not `"GLOBAL"`.

- [ ] **Step 3: Implement**

Replace the Global-mode block inside `to_tray_proxies`:

```rust
        // The core looks groups up by their exact, case-sensitive name; a
        // core without GLOBAL leaves `global` with an empty one.
        let global = &raw_proxies.global;
        if mode == Mode::Global && !global.name.is_empty() {
            let item = TrayProxyItem {
                current: global.now.clone(),
                all: global.all.clone(),
                r#type: global.r#type.clone(),
            };
            tray_proxies.insert(global.name.clone(), item);
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/tray/proxies.rs
git commit -F - <<'EOF'
fix(tray): select the GLOBAL group by its real name

The tray keyed the Global-mode group as "global" and selected nodes with
PUT /proxies/global. Mihomo resolves the group with a case-sensitive map
lookup in which only "GLOBAL" exists, so every Global-mode selection from
the tray failed with 404. The tray now uses the group's own name and
type, and adds no entry when the core has no GLOBAL group.
EOF
```

---

### Task 2: Leave hidden groups out of the tray menu (C2)

**Files:**

- Modify: `backend/tauri/src/core/tray/proxies.rs` (`to_tray_proxies` group loop, tests module)

**Interfaces:**

- Consumes: legacy `ProxyGroupItem.hidden: bool`.
- Produces: `to_tray_proxies` skips `groups` entries with `hidden == true`; `global` is never filtered.

- [ ] **Step 1: Write the failing test**

Add `use crate::core::clash::proxies::ProxyGroupItem;` at the top of the tests module (remove the same `use` inside `sample_proxies`), then add:

```rust
    /// The page filters hidden groups out; the tray must not list them
    /// either, while GLOBAL itself always stays in Global mode.
    #[test]
    fn hidden_groups_stay_out_of_the_tray() {
        let mut proxies = sample_proxies();
        proxies.global.hidden = true;
        proxies.groups.push(ProxyGroupItem {
            name: "Hidden".into(),
            r#type: "Selector".into(),
            all: vec!["node-a".into()],
            hidden: true,
            ..Default::default()
        });
        for mode in [Mode::Global, Mode::Rule, Mode::Script] {
            assert!(!to_tray_proxies(mode, &proxies).contains_key("Hidden"));
        }
        assert!(to_tray_proxies(Mode::Global, &proxies).contains_key("GLOBAL"));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray::proxies::tests::hidden_groups_stay_out_of_the_tray`
Expected: FAIL — the tray contains `"Hidden"`.

- [ ] **Step 3: Implement**

In `to_tray_proxies`, change the group loop header to:

```rust
        for raw_group in raw_proxies.groups.iter().filter(|group| !group.hidden) {
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/tray/proxies.rs
git commit -F - <<'EOF'
fix(tray): leave hidden groups out of the tray menu

A profile hides a group to keep it out of the UI, and the proxies page
filters such groups out, but the tray listed every group. The tray
projection now skips hidden groups too; GLOBAL stays in Global mode as
it does on the page.
EOF
```

---

### Task 3: Return provider and delay results as clash-api types (C3)

**Files:**

- Modify: `backend/tauri/src/core/clash/api.rs` (delete provider/delay DTOs)
- Modify: `backend/tauri/src/core/clash/proxies.rs` (provider input type; receives `proxy_item`/`proxy_items`)
- Modify: `backend/tauri/src/core/proxies.rs` (snapshot providers; remove `provider_item`, `proxy_item`, `proxy_items`; test fixture)
- Modify: `backend/tauri/src/client/clash_api.rs` (`proxy_providers`, `proxy_delay`, `group_delay`)
- Modify: `backend/tauri/src/ipc.rs` (three return types)
- Regenerate: `frontend/rpc/src/rpc-bindings.ts`, `frontend/query/src/query-bindings.ts` (and `frontend/rpc/src/tauri-bindings.ts` if it changes)
- Modify: `frontend/query/src/ipc/use-clash-proxies-provider.ts`, `frontend/query/src/ipc/index.ts`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx`
- Create: `frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`
- Modify (mocks): `frontend/nyanpasu/tests/dashboard-status-widgets.browser.test.tsx`, `frontend/nyanpasu/tests/providers-refresh.browser.test.tsx`, `frontend/nyanpasu/tests/widget-reference-config.browser.test.tsx`

**Interfaces:**

- Consumes: `ApiClient::proxy_snapshot()` → `ProxySnapshot { proxies: IndexMap<ProxyName, clash_api::Proxy>, providers: IndexMap<ProviderName, clash_api::ProxyProvider>, groups: Option<IndexMap<ProxyName, clash_api::Proxy>> }`.
- Produces:
  - `ProxiesClient::providers(&self) -> anyhow::Result<clash_api::IndexMap<ProviderName, ProxyProvider>>`
  - `NyanpasuClient::proxy_providers(&self) -> Result<IndexMap<ProviderName, ProxyProvider>>`
  - `NyanpasuClient::proxy_delay(&self, name: String, provider: Option<String>, url: Option<String>) -> Result<clash_api::Delay>`
  - `NyanpasuClient::group_delay(&self, group: String, url: Option<String>) -> Result<IndexMap<ProxyName, u16>>`
  - `Proxies::from_responses(inner_proxies: api::ProxiesRes, providers: &IndexMap<ProviderName, ProxyProvider>, group_list: Option<IndexMap<String, api::ProxyItem>>) -> Result<Proxies>` (interim; Task 4 replaces it)
  - `pub(crate) fn proxy_item(proxy: clash_api::Proxy) -> api::ProxyItem` and `pub(crate) fn proxy_items(...)` now live in `core/clash/proxies.rs` (interim; Task 4 deletes them)
  - TS: `clash_api_get_providers_proxies` returns `{ [key in ProviderName]: ProxyProvider_Serialize }` (no `providers` envelope); `ClashProviderProxies` aliases `ProxyProvider_Serialize`.

- [ ] **Step 1: Update the Rust tests (red: they no longer compile against the old API)**

In `backend/tauri/src/core/proxies.rs` tests, change the `providers()` fixture's `subscriptionInfo` to carry a negative value the old `usize` conversion rejected:

```rust
            serde_json::json!({"providers":{PROVIDER:{"name":PROVIDER,"type":"Proxy","vehicleType":"HTTP", "proxies":[{"name":NODE,"type":"Vless","udp":true,"history":[]}], "subscriptionInfo":{"Expire":42,"Upload":-1}}}}),
```

and in `cache_ttl_and_provider_metadata_are_shared` replace the provider assertion with:

```rust
        let providers = client.providers().await.unwrap();
        let info = providers[&clash_api::ProviderName::from(PROVIDER)]
            .subscription_info
            .as_ref()
            .unwrap();
        assert_eq!((info.expire, info.upload), (42, -1));
```

In `backend/tauri/src/core/clash/proxies.rs` tests, add at the top of the module:

```rust
    use clash_api::{ProviderName, ProxyProvider};
    use serde_json::json;

    fn provider(
        key: &str,
        vehicle: &str,
        proxies: serde_json::Value,
    ) -> (ProviderName, ProxyProvider) {
        let provider = serde_json::from_value(json!({
            "name": key, "type": "Proxy", "vehicleType": vehicle, "proxies": proxies
        }))
        .unwrap();
        (ProviderName::from(key), provider)
    }
```

Replace `resolves_provider_owned_proxy_with_metadata` with:

```rust
    #[test]
    fn resolves_provider_owned_proxy_with_metadata() {
        let providers = IndexMap::from([provider(
            "subscription",
            "HTTP",
            json!([{"name": "provider-node", "type": "Vless", "udp": true, "history": []}]),
        )]);

        let provider_proxies = provider_proxy_map(&providers);
        let resolved = resolve_proxy("provider-node", &IndexMap::new(), &provider_proxies);

        assert_eq!(resolved.r#type, "Vless");
        assert!(resolved.udp);
        assert_eq!(resolved.provider.as_deref(), Some("subscription"));
    }
```

In `assembles_provider_owned_nodes_with_metadata_for_supported_vehicles`, iterate `for vehicle_type in ["HTTP", "File", "Inline"]`, build the provider node and the provider with:

```rust
                let node: clash_api::Proxy = serde_json::from_value(json!({
                    "name": "provider-node", "type": proxy_type, "udp": true,
                    "history": [{"time": "2026-10-04T08:00:00Z", "delay": 42}],
                    "alive": true, "xudp": true, "tfo": true
                }))
                .unwrap();
                let providers = IndexMap::from([provider(
                    "provider",
                    vehicle_type,
                    json!([node.clone()]),
                )]);

                let proxies =
                    Proxies::from_responses(inner_proxies, &providers, None).unwrap();
                let mut expected = proxy_item(node);
                expected.provider = Some("provider".into());

                assert_eq!(proxies.groups[0].all, vec!["provider-node"]);
                assert_eq!(
                    serde_json::to_value(&proxies.nodes["provider-node"]).unwrap(),
                    serde_json::to_value(&expected).unwrap(),
                    "metadata must be preserved for {vehicle_type} / {proxy_type}"
                );
```

(keep the existing `inner_proxies` construction; delete the old `let mut node = api::ProxyItem {...}`, the `providers_proxies` value and the old assertions).

In `shares_one_node_record_across_groups_and_preserves_member_order`, replace `providers_proxies` with:

```rust
        let providers = IndexMap::from([provider(
            "sub",
            "Inline",
            json!([
                {"name": "provider-only", "type": "Trojan", "udp": false, "history": []},
                {"name": "shared-node", "type": "ProviderIgnored", "udp": false, "history": []}
            ]),
        )]);

        let proxies = Proxies::from_responses(inner_proxies, &providers, None).unwrap();
```

In `assemble`, replace the `providers` value and the call with:

```rust
        Proxies::from_responses(records(raw), &IndexMap::new(), listed).unwrap()
```

- [ ] **Step 2: Run the Rust tests to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::`
Expected: FAIL to compile (`provider_proxy_map`/`from_responses` take the old provider type; `providers()` returns `ProvidersProxiesRes`).

- [ ] **Step 3: Move `proxy_item`/`proxy_items` and take crate providers in `core/clash/proxies.rs`**

Cut `proxy_items` and `proxy_item` from `backend/tauri/src/core/proxies.rs` and paste them into `backend/tauri/src/core/clash/proxies.rs` above `provider_proxy_map`, unchanged except visibility (`pub(crate) fn`). Then replace `provider_proxy_map`:

```rust
/// Every proxy of an HTTP, File, or Inline provider by name, tagged with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies.
fn provider_proxy_map(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<String, api::ProxyItem> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            let mut proxy = proxy_item(proxy.clone());
            proxy.provider = Some(provider.as_str().to_owned());
            proxies.insert(proxy.name.clone(), proxy);
        }
    }
    proxies
}
```

Add `use clash_api::{ProviderName, ProxyProvider, VehicleType};`. In `Proxies::from_responses`, change the second parameter to `providers: &IndexMap<ProviderName, ProxyProvider>`, delete the `// 1. Include nodes from HTTP, File, and Inline providers.` filtering block, and build the map with `let provider_map = provider_proxy_map(providers);` (renumber the remaining step comments).

- [ ] **Step 4: Switch the actor snapshot and the facade**

In `backend/tauri/src/core/proxies.rs`:

```rust
use clash_api::{IndexMap, ProviderName, ProxyProvider};
// ...
use super::{
    actor_v2::{CoreClient, api::ApiClient},
    clash::{
        api,
        proxies::{Proxies, proxy_items},
    },
};

struct Snapshot {
    api: ApiClient,
    proxies: Proxies,
    providers: IndexMap<ProviderName, ProxyProvider>,
    fetched: Instant,
    fingerprint: Vec<u8>,
}
```

In `State::refresh`, replace the body of the `async` block up to the fingerprint with:

```rust
            let response = api.proxy_snapshot().await?;
            let groups = response.groups.map(proxy_items);
            let proxies = api::ProxiesRes {
                proxies: proxy_items(response.proxies),
            };
            let proxies = Proxies::from_responses(proxies, &response.providers, groups)?;
            let providers = response.providers;
            let fingerprint = serde_json::to_vec(&(&proxies, &providers))?;
```

Change `ProxiesClient::providers` to return `Result<IndexMap<ProviderName, ProxyProvider>>` (body unchanged). Delete `provider_item`.

In `backend/tauri/src/core/clash/api.rs` delete `impl From<ProxyProviderItem> for ProxyItem`, `VehicleType`, `ProviderType` and its `Display` impl, `SubscriptionInfo`, `ProxyProviderItem`, `ProvidersProxiesRes`, `DelayRes`, and the `std::fmt` import if it becomes unused.

In `backend/tauri/src/client/clash_api.rs`:

```rust
use clash_api::{Delay, DelayQuery, IndexMap, ProviderName, ProxyName, ProxyProvider};
// remove `use indexmap::IndexMap;` and `DelayRes` from the `crate::core::clash::api` import

    pub async fn proxy_providers(&self) -> Result<IndexMap<ProviderName, ProxyProvider>> {
        self.inner.proxies.providers().await
    }

    pub async fn proxy_delay(
        &self,
        name: String,
        provider: Option<String>,
        url: Option<String>,
    ) -> Result<Delay> {
        let query = delay_query(url)?;
        let provider = provider.map(ProviderName::new);
        Ok(self
            .inner
            .core_api
            .api_client()
            .await?
            .proxy_delay(&ProxyName::new(name), provider.as_ref(), &query)
            .await?)
    }

    pub async fn group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<IndexMap<ProxyName, u16>> {
        let query = delay_query(url)?;
        Ok(self
            .inner
            .core_api
            .api_client()
            .await?
            .group_delay(&ProxyName::new(group), &query)
            .await?)
    }
```

In `backend/tauri/src/ipc.rs` change only the return types:

```rust
pub async fn clash_api_get_proxy_delay(/* unchanged params */) -> Result<clash_api::Delay> {
pub async fn clash_api_get_group_delay(/* unchanged params */) -> Result<IndexMap<clash_api::ProxyName, u16>> {
pub async fn clash_api_get_providers_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProviderName, clash_api::ProxyProvider>> {
```

- [ ] **Step 5: Run the Rust tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::`
Expected: PASS (including `cache_ttl_and_provider_metadata_are_shared` with `Upload: -1`).

- [ ] **Step 6: Regenerate bindings and check names**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`
Then run the grep from Global Constraints. Expected: `ProxyProvider_Serialize`, `SubscriptionInfo…` (PascalCase fields `Upload`, `Download`, `Total`, `Expire`), `Proxy_Serialize`, `Delay`; no `ProxyProviderItem`, `ProvidersProxiesRes`, `DelayRes`.

- [ ] **Step 7: Write the failing frontend test**

Create `frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`:

```tsx
import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { useProxiesSubscription } from '@/pages/(main)/main/providers/_modules/use-proxies-subscription'
import type { ClashProxiesProviderQueryItem } from '@nyanpasu/query'

const provider = (
  subscriptionInfo: ClashProxiesProviderQueryItem['subscriptionInfo'],
): ClashProxiesProviderQueryItem => ({
  name: 'sub',
  type: 'Proxy',
  vehicleType: 'HTTP',
  updatedAt: null,
  subscriptionInfo,
  proxyCount: 1,
})

test('a provider without a subscription reports none instead of throwing', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(provider(null)),
  )

  expect(result.current).toEqual({
    progress: 0,
    total: 0,
    used: 0,
    hasSubscriptionInfo: false,
  })
})

test('usage sums upload and download against the total', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(
      provider({ Upload: 10, Download: 30, Total: 200, Expire: 0 }),
    ),
  )

  expect(result.current).toEqual({
    progress: 20,
    total: 200,
    used: 40,
    hasSubscriptionInfo: true,
  })
})
```

- [ ] **Step 8: Run it to verify it fails**

Run: `pnpm exec vitest run --project browser frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`
Expected: FAIL — the first test throws reading `download` of `null` (or typecheck of the fixture fails until Step 9 updates the hook's type source).

- [ ] **Step 9: Migrate the provider hook and the subscription hook**

`frontend/query/src/ipc/use-clash-proxies-provider.ts`:

```ts
import { unwrapResult } from '@nyanpasu/rpc'
import { type ProxyProvider_Serialize } from '@nyanpasu/rpc/types'
// ...
export type ClashProxiesProviderQueryItem = Pick<
  ProxyProvider_Serialize,
  'name' | 'type' | 'vehicleType' | 'updatedAt' | 'subscriptionInfo'
> & {
  proxyCount: number
}
// ...
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(providersQuery))

      if (!result) return {} as ClashProxiesProviderQuery

      return Object.fromEntries(
        Object.entries(result)
          .filter(([, value]) =>
            ['http', 'file'].includes(value.vehicleType.toLowerCase()),
          )
          .map(([key, value]) => [
            key,
            {
              name: value.name,
              type: value.type,
              vehicleType: value.vehicleType,
              updatedAt: value.updatedAt,
              subscriptionInfo: value.subscriptionInfo,
              proxyCount: value.proxies.length,
            },
          ]),
      ) as ClashProxiesProviderQuery
    },
```

`frontend/query/src/ipc/index.ts`:

```ts
export type { ProxyProvider_Serialize as ClashProviderProxies } from '@nyanpasu/rpc/types'
```

`frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx` — replace the body of the `useMemo` callback:

```ts
let progress = 0
let total = 0
let used = 0

// A provider without a subscription header reports null usage.
const subscriptionInfo = data.subscriptionInfo
const hasSubscriptionInfo = subscriptionInfo != null

if (hasSubscriptionInfo) {
  total = subscriptionInfo.Total

  used = subscriptionInfo.Download + subscriptionInfo.Upload

  if (total > 0) {
    progress = clampPercentage((used / total) * 100)
  }
}

return {
  progress,
  total,
  used,
  hasSubscriptionInfo,
}
```

- [ ] **Step 10: Drop the `providers` envelope from test mocks**

In each `clash_api_get_providers_proxies` mock, return the map itself. Leave `clash_api_get_providers_rules` mocks unchanged.

`frontend/nyanpasu/tests/dashboard-status-widgets.browser.test.tsx`:

```ts
return {
  'Proxy A': {
    name: 'Proxy A',
    type: 'Proxy',
    vehicleType: 'HTTP',
    updatedAt: null,
    subscriptionInfo: null,
    proxies: [],
  },
} as T
```

`frontend/nyanpasu/tests/providers-refresh.browser.test.tsx`:

```ts
return {
  sub: {
    name: 'sub',
    type: 'Proxy',
    vehicleType: 'HTTP',
    updatedAt: '2026-09-30T00:00:00Z',
    // Only the delay history changes between fetches.
    proxies: [
      {
        name: 'a',
        type: 'Shadowsocks',
        history: [{ delay: providerFetches }],
      },
    ],
  },
}
```

`frontend/nyanpasu/tests/widget-reference-config.browser.test.tsx`:

```ts
        case 'clash_api_get_providers_proxies':
          return {
            SharedName: {
              name: 'SharedName',
              type: 'Proxy',
              proxies: [],
              vehicleType: 'HTTP',
            },
          } as T
```

- [ ] **Step 11: Run frontend checks**

Run: `pnpm exec vitest run --project browser frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx frontend/nyanpasu/tests/dashboard-status-widgets.browser.test.tsx frontend/nyanpasu/tests/providers-refresh.browser.test.tsx frontend/nyanpasu/tests/widget-reference-config.browser.test.tsx frontend/query/tests/proxy-delay-history.browser.test.tsx`
Expected: PASS.
Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 12: Commit**

```bash
git status --short
git add backend/tauri/src/core/clash/api.rs backend/tauri/src/core/clash/proxies.rs \
  backend/tauri/src/core/proxies.rs backend/tauri/src/client/clash_api.rs backend/tauri/src/ipc.rs \
  frontend/rpc/src/rpc-bindings.ts frontend/query/src/query-bindings.ts \
  frontend/query/src/ipc/use-clash-proxies-provider.ts frontend/query/src/ipc/index.ts \
  'frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx' \
  frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx \
  frontend/nyanpasu/tests/dashboard-status-widgets.browser.test.tsx \
  frontend/nyanpasu/tests/providers-refresh.browser.test.tsx \
  frontend/nyanpasu/tests/widget-reference-config.browser.test.tsx
# also add frontend/rpc/src/tauri-bindings.ts if `git status` shows it modified
git diff --cached --stat
git commit -F - <<'EOF'
refactor(proxies): return provider and delay results as clash-api types

The provider and delay RPCs re-declared clash-api's records in app DTOs.
The copies drifted: subscription usage was narrowed from i64 to usize,
so one provider reporting a negative value failed the whole proxy
refresh, and the duplicate VehicleType, ProviderType and
SubscriptionInfo names blocked exporting the crate's own types. The RPCs
now return the clash-api records directly; the provider list loses its
`providers` envelope, and a provider without a subscription now arrives
as null, which the providers page treats as having no usage.
EOF
```

---

### Task 4: Build the proxy view from clash-api records (C4)

**Files:**

- Rewrite: `backend/tauri/src/core/clash/proxies.rs` (types, assembly, tests)
- Modify: `backend/tauri/src/core/clash/api.rs` (delete `ProxiesRes`, `ProxyItemHistory`, `ProxyItem`)
- Modify: `backend/tauri/src/core/proxies.rs` (refresh; tests)
- Modify: `backend/tauri/src/core/tray/proxies.rs` (`TrayGroup`, projection, diff, selector, tests)
- Modify: `backend/tauri/src/core/tray/display.rs` (tests use `TrayGroup`)
- Regenerate: bindings
- Modify: `frontend/query/src/ipc/use-clash-proxies.ts`, `frontend/nyanpasu/src/components/proxies/group-delay.ts`, `frontend/nyanpasu/src/components/proxies/group-summary.tsx`, `frontend/nyanpasu/src/components/proxies/delay-history.tsx`, `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx`, `frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx`, `frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/index.tsx`
- Modify (fixtures/tests): `frontend/nyanpasu/perf/fixtures/proxies.ts`, `frontend/query/tests/proxy-delay-history.browser.test.tsx`, `frontend/nyanpasu/tests/proxy-group-delay.test.ts`

**Interfaces:**

- Consumes: Task 3's `ProxySnapshot` and `IndexMap<ProviderName, ProxyProvider>` snapshot providers.
- Produces (Rust, `crate::core::clash::proxies`):
  - `pub enum ProxyGroupKind { Selector, UrlTest /* "URLTest" */, Fallback, LoadBalance, Relay, Smart, Unknown(String) }`
  - `pub struct ProxyGroupCapabilities { pub select: bool, pub clear_fixed: bool }` (serde `clearFixed`)
  - `pub struct ProxyGroup { pub name: ProxyName, pub kind: ProxyGroupKind /* serde "type" */, pub all: Vec<ProxyName>, pub now: Option<ProxyName>, pub fixed: Option<ProxyName>, pub hidden: bool, pub icon: Option<String>, pub capabilities: ProxyGroupCapabilities }`
  - `pub struct Proxies { pub global: Option<ProxyGroup>, pub groups: Vec<ProxyGroup>, pub nodes: IndexMap<ProxyName, clash_api::Proxy> }`
  - `Proxies::from_responses(proxies: IndexMap<ProxyName, Proxy>, providers: &IndexMap<ProviderName, ProxyProvider>, group_list: Option<IndexMap<ProxyName, Proxy>>) -> anyhow::Result<Proxies>`
  - Tray: `pub(super) struct TrayGroup { pub(super) now: Option<String>, pub(super) all: Vec<String>, pub(super) selectable: bool }`, `pub(super) type TrayProxies = IndexMap<String, TrayGroup>`, `TrayGroup::of(&ProxyGroup)`.
- Produces (TS): `ClashProxiesQueryProxyItem = Proxy_Serialize`, `ClashProxiesQueryGroupItem = ProxyGroup`, `ClashProxiesQuery = Proxies_Serialize` with `global: ProxyGroup | null`.

- [ ] **Step 1: Write the new assembly module with its tests (red until callers migrate)**

Replace the whole of `backend/tauri/src/core/clash/proxies.rs` with:

```rust
//! The proxy view the tray and the frontend share, assembled from one read
//! of the core's `/proxies`, `/providers/proxies` and group list.
use anyhow::Result;
use clash_api::{IndexMap, ProviderName, Proxy, ProxyName, ProxyProvider, VehicleType};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A group's `type`, as the core reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum ProxyGroupKind {
    Selector,
    #[serde(rename = "URLTest")]
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Smart,
    /// A type this build does not know, kept verbatim.
    #[serde(untagged)]
    Unknown(String),
}

impl ProxyGroupKind {
    fn parse(value: &str) -> Self {
        match value {
            "Selector" => Self::Selector,
            "URLTest" => Self::UrlTest,
            "Fallback" => Self::Fallback,
            "LoadBalance" => Self::LoadBalance,
            "Relay" => Self::Relay,
            "Smart" => Self::Smart,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

/// What the running core lets a user do with a group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupCapabilities {
    /// `PUT /proxies/{group}` chooses a member.
    pub select: bool,
    /// `DELETE /proxies/{group}` returns a pinned group to automatic selection.
    pub clear_fixed: bool,
}

impl ProxyGroupCapabilities {
    /// Mihomo and Meow report `fixed` (empty while unpinned) on exactly the
    /// URLTest and Fallback groups they let a user pin; Clash-rs omits it
    /// and rejects selecting those groups.
    fn infer(kind: &ProxyGroupKind, record: &Proxy) -> Self {
        match kind {
            ProxyGroupKind::Selector => Self {
                select: true,
                clear_fixed: false,
            },
            ProxyGroupKind::UrlTest | ProxyGroupKind::Fallback => {
                let pinnable = record.fixed.is_some();
                Self {
                    select: pinnable,
                    clear_fixed: pinnable,
                }
            }
            _ => Self::default(),
        }
    }
}

/// A group's meaning, derived from its record in `Proxies::nodes`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: ProxyName,
    #[serde(rename = "type")]
    pub kind: ProxyGroupKind,
    /// Member names; look each node up in `Proxies::nodes`.
    pub all: Vec<ProxyName>,
    pub now: Option<ProxyName>,
    /// The member a user pinned; `None` while the core selects on its own.
    pub fixed: Option<ProxyName>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub capabilities: ProxyGroupCapabilities,
}

impl ProxyGroup {
    fn from_record(record: &Proxy) -> Self {
        let kind = ProxyGroupKind::parse(&record.proxy_type);
        let capabilities = ProxyGroupCapabilities::infer(&kind, record);
        Self {
            name: record.name.clone(),
            kind,
            all: record.all.clone().unwrap_or_default(),
            now: record.now.clone(),
            fixed: record
                .fixed
                .clone()
                .filter(|name| !name.as_str().is_empty()),
            hidden: record.hidden.unwrap_or(false),
            icon: record.icon.clone(),
            capabilities,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    /// The core's GLOBAL group; `None` when the core has none.
    pub global: Option<ProxyGroup>,
    pub groups: Vec<ProxyGroup>,
    /// Every `/proxies` entry plus every provider-owned node referenced by a
    /// group, keyed by name. A node that belongs to several groups still has
    /// exactly one entry here; groups reference it by name in `all`.
    pub nodes: IndexMap<ProxyName, Proxy>,
}

/// `/proxies` names a node's provider in `provider` or `provider-name`.
fn with_provider(mut proxy: Proxy) -> Proxy {
    proxy.provider = proxy
        .provider
        .take()
        .filter(|name| !name.is_empty())
        .or_else(|| proxy.provider_name.clone().filter(|name| !name.is_empty()));
    proxy
}

/// Every proxy of an HTTP, File, or Inline provider by name, tagged with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies.
fn provider_proxy_map(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<ProxyName, Proxy> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            let mut proxy = proxy.clone();
            proxy.provider = Some(provider.as_str().to_owned());
            proxies.insert(proxy.name.clone(), proxy);
        }
    }
    proxies
}

/// A group member found neither in /proxies nor in any provider.
fn unknown_proxy(name: &ProxyName) -> Proxy {
    Proxy {
        name: name.clone(),
        proxy_type: "Unknown".to_owned(),
        history: Vec::new(),
        extra: None,
        alive: None,
        udp: false,
        uot: None,
        xudp: None,
        tfo: None,
        mptcp: None,
        smux: None,
        interface: None,
        routing_mark: None,
        provider_name: None,
        dialer_proxy: None,
        id: None,
        now: None,
        all: None,
        test_url: None,
        expected_status: None,
        fixed: None,
        hidden: None,
        icon: None,
        empty_fallback: None,
        provider: None,
    }
}

impl Proxies {
    /// `group_list` is the core's own group list; `None` infers groups from
    /// every `/proxies` record with members.
    pub fn from_responses(
        proxies: IndexMap<ProxyName, Proxy>,
        providers: &IndexMap<ProviderName, ProxyProvider>,
        group_list: Option<IndexMap<ProxyName, Proxy>>,
    ) -> Result<Self> {
        let mut nodes: IndexMap<ProxyName, Proxy> = proxies
            .into_iter()
            .map(|(name, proxy)| (name, with_provider(proxy)))
            .collect();
        // A listed group replaces its /proxies record, so one group never
        // mixes the members of one read with the selection of the other.
        let mut group_records: IndexMap<ProxyName, Proxy> = match group_list {
            Some(groups) => {
                let groups: IndexMap<ProxyName, Proxy> = groups
                    .into_iter()
                    .map(|(name, group)| (name, with_provider(group)))
                    .collect();
                for (name, group) in &groups {
                    nodes.insert(name.clone(), group.clone());
                }
                groups
            }
            None => nodes
                .iter()
                .filter(|(_, proxy)| proxy.all.is_some())
                .map(|(name, proxy)| (name.clone(), proxy.clone()))
                .collect(),
        };
        let global = group_records.swap_remove(&ProxyName::from("GLOBAL"));

        for required in ["DIRECT", "REJECT"] {
            anyhow::ensure!(
                nodes.contains_key(&ProxyName::from(required)),
                "{required} is missing in /proxies"
            );
        }

        // GLOBAL only orders the groups it lists, never decides which
        // groups exist; the rest follow by name.
        let mut ordered = Vec::with_capacity(group_records.len());
        if let Some(names) = global.as_ref().and_then(|group| group.all.as_ref()) {
            for name in names {
                if let Some(group) = group_records.swap_remove(name) {
                    ordered.push(group);
                }
            }
        }
        let mut remaining: Vec<_> = group_records.into_values().collect();
        remaining.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        ordered.extend(remaining);

        // Group members missing from /proxies (provider-owned nodes) are
        // added once; a node shared by several groups keeps a single entry.
        let provider_proxies = provider_proxy_map(providers);
        let mut convert = |record: Proxy| {
            for name in record.all.iter().flatten() {
                if !nodes.contains_key(name) {
                    let node = provider_proxies
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| unknown_proxy(name));
                    nodes.insert(name.clone(), node);
                }
            }
            ProxyGroup::from_record(&record)
        };
        let groups = ordered.into_iter().map(&mut convert).collect();
        let global = global.map(&mut convert);

        Ok(Proxies {
            global,
            groups,
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(value: serde_json::Value) -> Proxy {
        serde_json::from_value(value).unwrap()
    }

    fn item(name: &str, kind: &str, all: Option<Vec<&str>>, now: Option<&str>) -> Proxy {
        record(json!({
            "name": name, "type": kind, "udp": false, "history": [], "all": all, "now": now
        }))
    }

    fn name(value: &str) -> ProxyName {
        ProxyName::from(value)
    }

    fn provider(key: &str, vehicle: &str, proxies: Vec<Proxy>) -> (ProviderName, ProxyProvider) {
        let provider = serde_json::from_value(json!({
            "name": key, "type": "Proxy", "vehicleType": vehicle, "proxies": proxies
        }))
        .unwrap();
        (ProviderName::from(key), provider)
    }

    fn records(groups: &[Proxy]) -> IndexMap<ProxyName, Proxy> {
        let mut proxies = IndexMap::from([
            (name("DIRECT"), item("DIRECT", "Direct", None, None)),
            (name("REJECT"), item("REJECT", "Reject", None, None)),
        ]);
        proxies.extend(
            groups
                .iter()
                .map(|group| (group.name.clone(), group.clone())),
        );
        proxies
    }

    fn assemble(raw: &[Proxy], listed: Option<&[Proxy]>) -> Proxies {
        let listed = listed.map(|groups| {
            groups
                .iter()
                .map(|group| (group.name.clone(), group.clone()))
                .collect()
        });
        Proxies::from_responses(records(raw), &IndexMap::new(), listed).unwrap()
    }

    fn names(proxies: &Proxies) -> Vec<&str> {
        proxies.groups.iter().map(|g| g.name.as_str()).collect()
    }

    /// A group record with `fields` merged over a Selector with one member.
    fn group_with(fields: serde_json::Value) -> ProxyGroup {
        let mut value = json!({
            "name": "G", "type": "Selector", "udp": false, "history": [], "all": ["a"]
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        ProxyGroup::from_record(&record(value))
    }

    #[test]
    fn assembles_provider_owned_nodes_with_metadata_for_supported_vehicles() {
        for vehicle_type in ["HTTP", "File", "Inline"] {
            for proxy_type in ["Vless", "Trojan", "Hysteria2"] {
                let node = record(json!({
                    "name": "provider-node", "type": proxy_type, "udp": true,
                    "history": [{"time": "2026-10-04T08:00:00Z", "delay": 42}],
                    "alive": true, "xudp": true, "tfo": true,
                    "testUrl": "https://example.com/204", "dialer-proxy": "relay"
                }));
                let group = item(
                    "PROXY",
                    "Selector",
                    Some(vec!["provider-node"]),
                    Some("provider-node"),
                );
                let providers =
                    IndexMap::from([provider("provider", vehicle_type, vec![node.clone()])]);

                let proxies = Proxies::from_responses(records(&[group]), &providers, None).unwrap();
                let mut expected = node;
                expected.provider = Some("provider".into());

                assert_eq!(proxies.groups[0].all, [name("provider-node")]);
                assert_eq!(
                    proxies.nodes[&name("provider-node")],
                    expected,
                    "metadata must be preserved for {vehicle_type} / {proxy_type}"
                );
            }
        }
    }

    #[test]
    fn compatible_providers_do_not_supply_members() {
        let group = item("PROXY", "Selector", Some(vec!["grouped"]), None);
        let providers = IndexMap::from([provider(
            "PROXY",
            "Compatible",
            vec![item("grouped", "Vless", None, None)],
        )]);
        let proxies = Proxies::from_responses(records(&[group]), &providers, None).unwrap();
        assert_eq!(proxies.nodes[&name("grouped")].proxy_type, "Unknown");
    }

    /// A node shared by two groups is stored once in `nodes`; group `all`
    /// keeps member names in order; a member absent from `/proxies` and from
    /// any provider falls back to the Unknown placeholder.
    #[test]
    fn shares_one_node_record_across_groups_and_preserves_member_order() {
        let raw = [
            item("GLOBAL", "Selector", Some(vec!["GroupA", "GroupB"]), Some("GroupA")),
            item(
                "GroupA",
                "Selector",
                Some(vec!["shared-node", "provider-only", "a-only"]),
                Some("shared-node"),
            ),
            item(
                "GroupB",
                "Selector",
                Some(vec!["shared-node", "provider-only", "totally-unknown"]),
                Some("shared-node"),
            ),
            item("shared-node", "VmessSharedMarker", None, None),
            item("a-only", "Vmess", None, None),
        ];
        let providers = IndexMap::from([provider(
            "sub",
            "Inline",
            vec![
                item("provider-only", "Trojan", None, None),
                item("shared-node", "ProviderIgnored", None, None),
            ],
        )]);

        let proxies = Proxies::from_responses(records(&raw), &providers, None).unwrap();

        let members = |group: &str| {
            proxies
                .groups
                .iter()
                .find(|g| g.name.as_str() == group)
                .unwrap()
                .all
                .clone()
        };
        assert_eq!(
            members("GroupA"),
            [name("shared-node"), name("provider-only"), name("a-only")]
        );
        assert_eq!(
            members("GroupB"),
            [name("shared-node"), name("provider-only"), name("totally-unknown")]
        );

        // Serialized once: the marker only lives on the full node record.
        let serialized = serde_json::to_string(&proxies).unwrap();
        assert_eq!(serialized.matches("VmessSharedMarker").count(), 1);
        assert_eq!(serialized.matches("Trojan").count(), 1);
        assert_eq!(proxies.nodes[&name("shared-node")].proxy_type, "VmessSharedMarker");
        assert!(proxies.nodes[&name("shared-node")].provider.is_none());

        let provider_node = &proxies.nodes[&name("provider-only")];
        assert_eq!(provider_node.proxy_type, "Trojan");
        assert_eq!(provider_node.provider.as_deref(), Some("sub"));

        let unknown_node = &proxies.nodes[&name("totally-unknown")];
        assert_eq!(unknown_node.proxy_type, "Unknown");
        assert!(unknown_node.history.is_empty());
    }

    #[test]
    fn listed_groups_decide_membership_and_replace_proxy_records() {
        let old = item("Foo", "Selector", Some(vec!["DIRECT"]), Some("DIRECT"));
        let listed = item("Foo", "LoadBalance", Some(vec!["Bar", "missing"]), None);
        let bar = item("Bar", "FutureGroup", Some(vec![]), None);
        let extra = item("extra", "Selector", Some(vec![]), None);
        let result = assemble(&[old, extra], Some(&[listed, bar]));
        assert_eq!(names(&result), ["Bar", "Foo"]);
        assert_eq!(result.nodes[&name("Foo")].proxy_type, "LoadBalance");
        assert_eq!(result.nodes[&name("missing")].proxy_type, "Unknown");
        assert!(result.nodes.contains_key(&name("extra")));
    }

    #[test]
    fn an_empty_group_list_is_not_replaced_by_inference() {
        let raw = [
            item("GLOBAL", "Selector", Some(vec!["Foo"]), None),
            item("Foo", "Selector", Some(vec![]), None),
        ];
        let result = assemble(&raw, Some(&[]));
        assert!(result.groups.is_empty());
        assert!(result.global.is_none());
        assert!(result.nodes.contains_key(&name("Foo")));
    }

    #[test]
    fn global_only_orders_groups_whether_listed_or_inferred() {
        let foo = item("Foo", "Selector", Some(vec!["Bar", "Foo"]), None);
        let bar = item("Bar", "UnknownGroup", Some(vec![]), None);
        let lower = item("global", "Fallback", Some(vec!["Foo"]), None);
        let global = item(
            "GLOBAL",
            "Selector",
            Some(vec!["Foo", "Foo", "DIRECT", "missing", "GLOBAL"]),
            None,
        );
        for listed in [false, true] {
            for has_global in [false, true] {
                let mut groups = vec![foo.clone(), bar.clone(), lower.clone()];
                if has_global {
                    groups.push(global.clone());
                }
                for reverse in [false, true] {
                    if reverse {
                        groups.reverse();
                    }
                    let result = assemble(&groups, listed.then_some(groups.as_slice()));
                    let expected = if has_global {
                        ["Foo", "Bar", "global"]
                    } else {
                        ["Bar", "Foo", "global"]
                    };
                    assert_eq!(names(&result), expected);
                    assert_eq!(result.global.is_none(), !has_global);
                    assert_eq!(
                        result.nodes[&name("Foo")].all.as_ref().unwrap(),
                        &[name("Bar"), name("Foo")]
                    );
                }
            }
        }
    }

    #[test]
    fn capabilities_follow_the_type_and_the_fixed_field() {
        let cases = [
            ("Selector", None, (true, false)),
            ("Selector", Some(""), (true, false)),
            ("URLTest", None, (false, false)),
            ("URLTest", Some(""), (true, true)),
            ("URLTest", Some("a"), (true, true)),
            ("Fallback", None, (false, false)),
            ("Fallback", Some(""), (true, true)),
            ("LoadBalance", None, (false, false)),
            ("Relay", None, (false, false)),
            ("Smart", None, (false, false)),
            ("Weighted", Some(""), (false, false)),
        ];
        for (kind, fixed, (select, clear_fixed)) in cases {
            let mut fields = json!({ "type": kind });
            if let Some(fixed) = fixed {
                fields["fixed"] = json!(fixed);
            }
            assert_eq!(
                group_with(fields).capabilities,
                ProxyGroupCapabilities {
                    select,
                    clear_fixed
                },
                "{kind} with fixed {fixed:?}"
            );
        }
    }

    #[test]
    fn group_fields_are_normalized_from_the_record() {
        let unpinned = group_with(json!({"type": "URLTest", "fixed": "", "now": "a"}));
        assert_eq!(unpinned.kind, ProxyGroupKind::UrlTest);
        assert_eq!(unpinned.fixed, None);
        assert!(!unpinned.hidden);

        // URLTest keeps a pin while it routes through another member.
        let pinned = group_with(json!({
            "type": "URLTest", "fixed": "a", "now": "b", "hidden": true, "icon": "i"
        }));
        assert_eq!(pinned.fixed, Some(name("a")));
        assert_eq!(pinned.now, Some(name("b")));
        assert!(pinned.hidden);
        assert_eq!(pinned.icon.as_deref(), Some("i"));

        let memberless = group_with(json!({"type": "LoadBalance", "all": null}));
        assert!(memberless.all.is_empty());
        assert_eq!(memberless.now, None);
    }

    #[test]
    fn the_group_type_serializes_as_the_core_reports_it() {
        for (raw, kind) in [
            ("Selector", ProxyGroupKind::Selector),
            ("URLTest", ProxyGroupKind::UrlTest),
            ("Fallback", ProxyGroupKind::Fallback),
            ("LoadBalance", ProxyGroupKind::LoadBalance),
            ("Relay", ProxyGroupKind::Relay),
            ("Smart", ProxyGroupKind::Smart),
            ("Weighted", ProxyGroupKind::Unknown("Weighted".into())),
        ] {
            assert_eq!(ProxyGroupKind::parse(raw), kind);
            assert_eq!(serde_json::to_value(&kind).unwrap(), json!(raw));
        }
    }

    #[test]
    fn node_provider_prefers_provider_then_provider_name() {
        let cases = [
            (json!({"provider": "p", "provider-name": "q"}), Some("p")),
            (json!({"provider": "", "provider-name": "q"}), Some("q")),
            (json!({"provider-name": ""}), None),
            (json!({}), None),
        ];
        for (fields, expected) in cases {
            let mut value = json!({"name": "n", "type": "Vless", "udp": false, "history": []});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let result = assemble(&[record(value)], None);
            assert_eq!(result.nodes[&name("n")].provider.as_deref(), expected);
        }
    }
}
```

- [ ] **Step 2: Run the module tests to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::clash::proxies`
Expected: FAIL to compile — `core/proxies.rs` still calls `proxy_items` and the tray still reads `ProxyGroupItem` fields.

- [ ] **Step 3: Migrate the actor and delete the legacy DTOs**

In `backend/tauri/src/core/proxies.rs`, change the `super::` import to `clash::proxies::Proxies` (drop `api` and `proxy_items`), and in `State::refresh` replace the lines from `let response` through `let fingerprint` with:

```rust
            let response = api.proxy_snapshot().await?;
            let proxies =
                Proxies::from_responses(response.proxies, &response.providers, response.groups)?;
            let providers = response.providers;
            let fingerprint = serde_json::to_vec(&(&proxies, &providers))?;
```

In its tests, add `use clash_api::ProxyName;` and update the three assertions:

```rust
        assert_eq!(proxies.groups[0].all[0].as_str(), NODE);
        assert_eq!(
            proxies.nodes[&ProxyName::from(NODE)].provider.as_deref(),
            Some(PROVIDER)
        );
```

```rust
        assert_eq!(
            client.snapshot().groups[0].now.as_ref().map(ProxyName::as_str),
            Some(NODE)
        );
```

```rust
        assert_eq!(proxies.groups[0].name.as_str(), "listed");
        assert_eq!(
            proxies.nodes[&ProxyName::from(NODE)].provider.as_deref(),
            Some(PROVIDER)
        );
```

In `backend/tauri/src/core/clash/api.rs` delete `ProxiesRes`, `ProxyItemHistory` and `ProxyItem`.

- [ ] **Step 4: Migrate the tray projection**

In `backend/tauri/src/core/tray/proxies.rs`:

```rust
use crate::{
    client::effects::plan::TrayView,
    core::clash::proxies::{Proxies, ProxyGroup, ProxyGroupKind},
    log_err,
};
```

Replace `TrayProxyItem`, `TrayProxies` and `to_tray_proxies` with:

```rust
pub(super) struct TrayGroup {
    pub(super) now: Option<String>,
    pub(super) all: Vec<String>,
    /// Whether the core accepts choosing one of `all`.
    pub(super) selectable: bool,
}
pub(super) type TrayProxies = IndexMap<String, TrayGroup>;

impl TrayGroup {
    fn of(group: &ProxyGroup) -> Self {
        Self {
            now: group.now.as_ref().map(|name| name.as_str().to_owned()),
            all: group.all.iter().map(|name| name.as_str().to_owned()).collect(),
            selectable: matches!(
                group.kind,
                ProxyGroupKind::Selector | ProxyGroupKind::Fallback
            ),
        }
    }
}

/// The groups the tray lists, keyed by each group's real name.
fn to_tray_proxies(mode: Mode, proxies: &Proxies) -> TrayProxies {
    if !matches!(mode, Mode::Global | Mode::Rule | Mode::Script) {
        return TrayProxies::new();
    }
    // GLOBAL stays in Global mode even when hidden, as on the page.
    let global = proxies.global.as_ref().filter(|_| mode == Mode::Global);
    let groups = proxies.groups.iter().filter(|group| !group.hidden);
    global
        .into_iter()
        .chain(groups)
        .map(|group| (group.name.as_str().to_owned(), TrayGroup::of(group)))
        .collect()
}
```

In `diff_proxies`, compare selectability with the member list and rename the selection field:

```rust
        // check if the length of all list or the selectability is different
        if item.all.len() != old_item.all.len() || item.selectable != old_item.selectable {
            return TrayUpdateType::Full;
        }
```

and replace `item.current` / `old_item.current` with `item.now` / `old_item.now`.

In `platform_impl`, import `TrayGroup` instead of `TrayProxyItem`; in `generate_group_selector` take `group: &TrayGroup`, read `group.now.clone()` instead of `group.current.clone()`, and replace the type check with:

```rust
            if !group.selectable {
                sub_item_builder = sub_item_builder.enabled(false);
            }
```

- [ ] **Step 5: Migrate the tray tests**

In the `backend/tauri/src/core/tray/proxies.rs` tests module, replace the `use crate::core::clash::proxies::ProxyGroupItem;` import, `selecting`, `sample_proxies`, `global_mode_selects_global_by_its_real_name` and `hidden_groups_stay_out_of_the_tray` with:

```rust
    use crate::core::clash::proxies::ProxyGroupCapabilities;

    fn selecting(now: Option<&str>) -> TrayProxies {
        TrayProxies::from([(
            "Proxy".to_owned(),
            TrayGroup {
                now: now.map(str::to_owned),
                all: vec!["a".to_owned(), "b".to_owned()],
                selectable: true,
            },
        )])
    }

    fn group(name: &str, all: &[&str], now: &str) -> ProxyGroup {
        ProxyGroup {
            name: name.into(),
            kind: ProxyGroupKind::Selector,
            all: all.iter().map(|&member| member.into()).collect(),
            now: Some(now.into()),
            fixed: None,
            hidden: false,
            icon: None,
            capabilities: ProxyGroupCapabilities {
                select: true,
                clear_fixed: false,
            },
        }
    }

    fn sample_proxies() -> Proxies {
        Proxies {
            global: Some(group("GLOBAL", &["GroupA"], "GroupA")),
            groups: vec![group("GroupA", &["node-a"], "node-a")],
            ..Default::default()
        }
    }

    /// The core looks a group up by its exact, case-sensitive name, so the
    /// tray must select "GLOBAL", never a lowercase alias; a core without
    /// GLOBAL gets no entry.
    #[test]
    fn global_mode_selects_global_by_its_real_name() {
        let mut proxies = sample_proxies();
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GLOBAL", "GroupA"]
        );

        proxies.global = None;
        let tray = to_tray_proxies(Mode::Global, &proxies);
        assert_eq!(
            tray.keys().map(String::as_str).collect::<Vec<_>>(),
            ["GroupA"]
        );
    }

    /// The page filters hidden groups out; the tray must not list them
    /// either, while GLOBAL itself always stays in Global mode.
    #[test]
    fn hidden_groups_stay_out_of_the_tray() {
        let mut proxies = sample_proxies();
        proxies.global.as_mut().unwrap().hidden = true;
        let mut hidden = group("Hidden", &["node-a"], "node-a");
        hidden.hidden = true;
        proxies.groups.push(hidden);
        for mode in [Mode::Global, Mode::Rule, Mode::Script] {
            assert!(!to_tray_proxies(mode, &proxies).contains_key("Hidden"));
        }
        assert!(to_tray_proxies(Mode::Global, &proxies).contains_key("GLOBAL"));
    }

    /// Enabling or disabling items changes the menu's structure, which only
    /// a rebuild shows.
    #[test]
    fn a_selectability_change_needs_a_rebuild() {
        let open = selecting(Some("a"));
        let mut locked = selecting(Some("a"));
        locked["Proxy"].selectable = false;
        assert_eq!(diff_proxies(&open, &locked), TrayUpdateType::Full);
    }
```

In `global_mode_adds_a_global_entry_rule_mode_does_not`, keep the Task 1 assertions (`"GLOBAL"` key, `all == ["GroupA"]`) and the rule-mode assertions unchanged.

In `backend/tauri/src/core/tray/display.rs` tests, import `super::super::proxies::TrayGroup` instead of `TrayProxyItem` and build:

```rust
            TrayGroup {
                now: Some(proxy.to_owned()),
                all: vec!["a".to_owned(), "b".to_owned()],
                selectable: true,
            },
```

- [ ] **Step 6: Run the Rust tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::`
Expected: PASS.

- [ ] **Step 7: Regenerate bindings and check names**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`, then the grep from Global Constraints. Expected: `Proxies_Serialize` with `global: ProxyGroup | null` and `nodes: { [key in ProxyName]: Proxy_Serialize }`; `ProxyGroup`, `ProxyGroupKind`, `ProxyGroupCapabilities`, `DelayHistory` exported; no `ProxyItem`, `ProxyGroupItem`, `ProxyItemHistory`.

- [ ] **Step 8: Migrate the frontend**

`frontend/query/src/ipc/use-clash-proxies.ts` — replace the type imports and aliases:

```ts
import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'

// ...
export type ClashProxiesQueryProxyItem = Proxy_Serialize

export type ClashProxiesQueryGroupItem = ProxyGroup

export type ClashProxiesQuery = Proxies_Serialize
```

and in `withDelaySample` change `satisfies ProxyItemHistory[]` to `satisfies DelayHistory[]`. Keep the hidden filter.

`frontend/nyanpasu/src/components/proxies/group-delay.ts`:

```ts
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'

export function getGroupSelectedDelay(
  group: ProxyGroup,
  proxies: Proxies_Serialize,
): number | undefined {
```

and change the node annotation to `const node: Proxy_Serialize | undefined = proxies.nodes[name]`.

`frontend/nyanpasu/src/components/proxies/group-summary.tsx`: import `Proxies_Serialize, ProxyGroup` and type the prop as `group: ProxyGroup`.

`frontend/nyanpasu/src/components/proxies/delay-history.tsx` declares a component named `DelayHistory`, so import the sample type under an alias and replace the three `ProxyItemHistory` annotations with it:

```ts
import type { DelayHistory as DelayHistorySample } from '@nyanpasu/rpc/types'
// ...
export function DelayHistoryBar({ history }: { history: DelayHistorySample[] }) {
// ...
function DelayHistoryContent({ history }: { history: DelayHistorySample[] }) {
// ...
  history?: DelayHistorySample[]
```

`frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx` and `frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx`, inside `currentGroup`'s `useMemo`:

```ts
if (proxyMode.global) {
  return proxies?.global ?? undefined
}
```

`frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/index.tsx`, in `ProxyButton`:

```ts
const currentDelay = useMemo(() => {
  const history = nodes[proxy.name]?.history ?? []

  if (history.length > 0) {
    return history[history.length - 1].delay
  }

  if (proxy.now) {
    const nodeDelay = nodes[proxy.now]?.history.at(-1)?.delay

    if (nodeDelay !== undefined) {
      return nodeDelay
    }
  }

  return -1
}, [proxy.name, proxy.now, nodes])
```

- [ ] **Step 9: Migrate frontend fixtures and tests**

`frontend/nyanpasu/perf/fixtures/proxies.ts`:

```ts
import type {
  ClashProxiesQuery,
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
} from '@nyanpasu/query'

const PROXY_TYPES = ['Shadowsocks', 'Vmess', 'Trojan', 'Hysteria2']

const record = (name: string, type: string): ClashProxiesQueryProxyItem => ({
  name,
  type,
  udp: false,
  history: [],
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})

// Every group lists every node, like a subscription whose groups all select
// from the full node list. Names carry a flag emoji, as real ones often do.
export function createProxiesFixture({
  groups,
  nodes,
}: {
  groups: number
  nodes: number
}): ClashProxiesQuery {
  const nodeMap: ClashProxiesQuery['nodes'] = {}
  const names: string[] = []

  for (let i = 0; i < nodes; i++) {
    const type = PROXY_TYPES[i % PROXY_TYPES.length]
    const name = `🇯🇵 Node ${i} | ${type} relay-${i % 17}`
    names.push(name)
    nodeMap[name] = {
      ...record(name, type),
      udp: i % 2 === 0,
      xudp: i % 3 === 0,
      tfo: i % 5 === 0,
      alive: true,
      history: Array.from({ length: 10 }, (_, k) => ({
        time: new Date(1_700_000_000_000 + k * 1000).toISOString(),
        delay: (i * 7 + k * 13) % 600,
      })),
    }
  }

  const groupItems = Array.from(
    { length: groups },
    (_, g) =>
      ({
        name: `Group ${g}`,
        type: 'Selector',
        all: names,
        now: names[(g * 37) % nodes],
        fixed: null,
        hidden: false,
        icon: null,
        capabilities: { select: true, clearFixed: false },
      }) satisfies ClashProxiesQueryGroupItem,
  )

  for (const group of groupItems) {
    nodeMap[group.name] = {
      ...record(group.name, 'Selector'),
      all: group.all,
      now: group.now,
    }
  }

  return {
    global: groupItems[0],
    groups: groupItems,
    nodes: nodeMap,
  }
}
```

`frontend/query/tests/proxy-delay-history.browser.test.tsx` — replace the type import and the two builders:

```ts
import {
  type Proxies_Serialize,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@nyanpasu/rpc/types'

// ...
const node = (name: string): Proxy_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  history: [{ time: '2026-09-10T00:00:00Z', delay: 42 }],
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})

const group = (name: string): ProxyGroup => ({
  name,
  type: 'Selector',
  all: ['tested', 'other'],
  now: 'tested',
  fixed: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: false },
})
```

`frontend/nyanpasu/tests/proxy-group-delay.test.ts` — replace the whole file:

```ts
import { expect, test } from 'vitest'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import { getGroupSelectedDelay } from '../src/components/proxies/group-delay.ts'

const node = (name: string, delays: number[] = []): Proxy_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  history: delays.map((delay) => ({ time: '', delay })),
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})
// A group's record, as it appears among the nodes.
const groupRecord = (
  name: string,
  now: string | null,
  all: string[],
): Proxy_Serialize => ({ ...node(name), type: 'Selector', now, all })
const group = (
  name: string,
  now: string | null,
  all: string[],
): ProxyGroup => ({
  name,
  type: 'Selector',
  all,
  now,
  fixed: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: false },
})
const snapshot = (
  global: ProxyGroup | null,
  groups: ProxyGroup[],
  nodes: Record<string, Proxy_Serialize>,
): Proxies_Serialize => ({ global, groups, nodes })

test('uses the selected member latest measurement, including failure and no history', () => {
  const other = node('other', [10])
  const selected = node('selected', [30, 80])
  const root = group('root', 'selected', ['other', 'selected'])
  const nodes = { other, selected }
  const data = snapshot(root, [root], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(80)
  selected.history.push({ time: '', delay: 0 })
  expect(getGroupSelectedDelay(root, data)).toBe(0)
  selected.history = []
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'other'
  expect(getGroupSelectedDelay(root, data)).toBe(10)
})

test('resolves nested and global selections through the shared node record', () => {
  const leaf = node('leaf', [42])
  const auto = groupRecord('auto', 'leaf', ['leaf'])
  const root = group('root', 'auto', ['auto'])
  const nodes = { auto, leaf }
  const data = snapshot(root, [root, group('auto', 'leaf', ['leaf'])], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(42)
  expect(getGroupSelectedDelay(data.global!, data)).toBe(42)
  // Only one `leaf` object exists, so a new sample is visible from every path.
  leaf.history.push({ time: '', delay: 75 })
  expect(getGroupSelectedDelay(root, data)).toBe(75)
})

test('resolves a hidden group that is not listed at the top level', () => {
  const leaf = node('leaf', [42])
  const hidden = groupRecord('hidden', 'leaf', ['leaf'])
  const root = group('root', 'hidden', ['hidden'])
  const nodes = { hidden, leaf }
  const data = snapshot(root, [root], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(42)
})

test('missing selections, unresolved nodes, and cycles have no selected latency', () => {
  const root = group('root', null, [])
  const data = snapshot(root, [root], {})
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'missing'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'root'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  data.groups.push(group('nested', 'root', ['root']))
  data.nodes.nested = groupRecord('nested', 'root', ['root'])
  root.now = 'nested'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
})
```

- [ ] **Step 10: Run frontend checks**

Run: `pnpm typecheck`
Expected: PASS.
Run: `pnpm exec vitest run --project unit frontend/nyanpasu/tests/proxy-group-delay.test.ts`
Run: `pnpm exec vitest run --project browser frontend/query/tests/proxy-delay-history.browser.test.tsx frontend/nyanpasu/tests/navbar.browser.test.tsx`
Expected: PASS.

- [ ] **Step 11: Commit**

```bash
git status --short
git add backend/tauri/src/core/clash/api.rs backend/tauri/src/core/clash/proxies.rs \
  backend/tauri/src/core/proxies.rs backend/tauri/src/core/tray/proxies.rs \
  backend/tauri/src/core/tray/display.rs \
  frontend/rpc/src/rpc-bindings.ts frontend/query/src/query-bindings.ts \
  frontend/query/src/ipc/use-clash-proxies.ts \
  frontend/nyanpasu/src/components/proxies/group-delay.ts \
  frontend/nyanpasu/src/components/proxies/group-summary.tsx \
  frontend/nyanpasu/src/components/proxies/delay-history.tsx \
  'frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx' \
  'frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx' \
  'frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/index.tsx' \
  frontend/nyanpasu/perf/fixtures/proxies.ts \
  frontend/query/tests/proxy-delay-history.browser.test.tsx \
  frontend/nyanpasu/tests/proxy-group-delay.test.ts
# also add frontend/rpc/src/tauri-bindings.ts if `git status` shows it modified
git diff --cached --stat
git commit -F - <<'EOF'
refactor(proxies): build the proxy view from clash-api records

The proxy view copied each clash-api record into a lossy app DTO, so the
page never received fields such as fixed, testUrl and extra, and the tray
and the page each guessed from the type string which groups accept a
selection. Nodes now keep the clash-api records, and each group is
described once in the pure assembly: its kind, its pin with mihomo's
empty-string sentinel normalized away, and what the core lets a user do
with it, inferred from whether the record reports a fixed field at all.
GLOBAL becomes optional instead of an empty-named placeholder. The tray
projects these groups and rebuilds when a group's selectability changes,
keeping its current selection gate until it reads the capabilities.
EOF
```

---

### Task 5: Enable tray node selection only where the core accepts it (C5)

**Files:**

- Modify: `backend/tauri/src/core/tray/proxies.rs` (`TrayGroup::of`, tests)

**Interfaces:**

- Consumes: `ProxyGroup.capabilities.select` from Task 4.
- Produces: `TrayGroup.selectable == group.capabilities.select`.

- [ ] **Step 1: Write the failing test**

Add to the tray tests module:

```rust
    /// Mihomo pins a URLTest group on selection, while Clash-rs rejects
    /// selecting a Fallback group; the tray follows what the core reports.
    #[test]
    fn only_groups_the_core_lets_a_user_select_are_selectable() {
        let mut proxies = sample_proxies();
        let mut pinnable = group("Auto", &["node-a"], "node-a");
        pinnable.kind = ProxyGroupKind::UrlTest;
        pinnable.capabilities.select = true;
        let mut automatic = group("Fallback", &["node-a"], "node-a");
        automatic.kind = ProxyGroupKind::Fallback;
        automatic.capabilities.select = false;
        proxies.groups = vec![pinnable, automatic];

        let tray = to_tray_proxies(Mode::Rule, &proxies);
        assert!(tray["Auto"].selectable);
        assert!(!tray["Fallback"].selectable);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray::proxies::tests::only_groups_the_core_lets_a_user_select_are_selectable`
Expected: FAIL — `Auto` is not selectable under the type-based gate.

- [ ] **Step 3: Implement**

In `TrayGroup::of`:

```rust
            selectable: group.capabilities.select,
```

`ProxyGroupKind` is now unused outside the tests: change the top-level import to `core::clash::proxies::{Proxies, ProxyGroup}`, and change the tests module's import to `use crate::core::clash::proxies::{ProxyGroupCapabilities, ProxyGroupKind};`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/tray/proxies.rs
git commit -F - <<'EOF'
fix(tray): enable node selection only where the core accepts it

The tray enabled Selector and Fallback groups by their type name. Mihomo
and Meow also let a user pin a URLTest group, which the tray disabled,
and Clash-rs rejects selecting a Fallback group, which the tray offered
and then failed with 404. The tray now enables exactly the groups whose
capabilities say the running core accepts a selection.
EOF
```

---

### Task 6: Branch verification and review loop

**Files:** none beyond fixes the review requires.

- [ ] **Step 1: Run the full checks**

```bash
pnpm lint:rustfmt
pnpm lint:clippy
pnpm test:backend
pnpm typecheck
pnpm test:frontend
pnpm lint
deno task lint:architecture-ledger
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings
git status --short   # bindings must be unchanged after the export
```

Expected: all pass and `git status` shows only `.pnpm-store/`. If a backend or frontend test fails, rerun that test after `git switch --detach main` (then `git switch -` back); a failure that also occurs on `main` is pre-existing — record it in the final report instead of fixing it here.

- [ ] **Step 2: Run the review loop**

Invoke the `ccg:review` skill on the branch diff against `main`. For each Critical/High finding: fix it in the commit that introduced the code (`git commit --fixup=<sha>`, then `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash main`; the branch is unpushed), rerun the affected checks from Step 1, and invoke `ccg:review` again. Stop when a review reports no Critical/High findings. Report Medium/Low findings to the user without fixing them unless they ask.

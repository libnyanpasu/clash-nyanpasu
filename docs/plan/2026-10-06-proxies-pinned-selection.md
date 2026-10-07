# Proxies Pinned Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the review minors left by the proxies API unification, then gate node selection on core capabilities on every surface, add a clear-pin operation, show pinned members, report selection failures, and keep a group's pin through a latency test.

**Architecture:** Phase 1 is four independent follow-up commits on `fix/proxies-unification-followups`. Phase 2, stacked on `feat/proxies-pinned-selection`, adds `ProxiesActor` `ClearFixed` (sharing the post-mutation path with `Select`), a `clear_proxy_fixed` RPC, and frontend/tray changes that read `ProxyGroup.capabilities` and `ProxyGroup.fixed` from the existing snapshot. No surface infers capabilities on its own.

**Tech Stack:** Rust (ractor actor, axum test fixtures, tauri menus, rust-i18n), TypeScript/React (TanStack Query, Vitest browser tests, Paraglide), specta-generated bindings.

**Spec:** `docs/spec/2026-10-06-proxies-pinned-selection/design.md`

## Global Constraints

- Work in the main checkout (`/Users/a632079/Programs/clash-nyanpasu`). Phase 1 branch: `fix/proxies-unification-followups` (from `main` `6e30d919a`). Phase 2 branch: `feat/proxies-pinned-selection`, created from the Phase 1 head after Phase 1 passes review.
- Follow `AGENTS.md` and `docs/development/*.md`. No new globals; no new `tauri` dependency in `NyanpasuClient`, typed clients, actors, or pure services. Tray code (`core/tray/`) is GUI code and may use Tauri.
- Do not hand-edit generated files (`frontend/rpc/src/rpc-bindings.ts`, `frontend/rpc/src/tauri-bindings.ts`, `frontend/query/src/query-bindings.ts`, `frontend/nyanpasu/src/paraglide/**`). Regenerate bindings with `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`. Paraglide output is gitignored; it regenerates when Vite runs (`pnpm web:build`).
- Rust test command shape: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib <filter>`. Frontend tests run from the repo root: `pnpm exec vitest run <file>`.
- `backend/tauri/tmp/dist` must exist for any tauri-crate cargo command (it does in the main checkout).
- Stage explicit paths only (never `git add .`/`-A`). One commit per task, message exactly as given, with the given body. No `Co-Authored-By` trailers.
- Known pre-existing failure: `connection_policy::profile_policy_and_noop_gates_do_not_acquire_a_source` fails in the full lib run on `main`; ignore it.
- New i18n keys go beside related keys in all five locale files (`en`, `ko`, `ru`, `zh-cn`, `zh-tw`), in the same position in each file. Frontend keys go right after `proxies_group_delay_test_pending_title`; tray keys go right after `"select_proxy"` inside `"tray"`.
- React components: keep existing `data-slot`s; add the `data-slot`s named in tasks; prefer `@nyanpasu/ui` components; separate responsibilities with one blank line (see `docs/development/typescript.md`).
- Constants: `PINNED_GROUP_DELAY_CONCURRENCY = 8`. Tray pin suffix: `" 📌"`. Unfix item id prefix: `"proxy_unfix:"`. Degradation message prefix for clear-pin: `"pinned selection cleared, but"`.

## Review Focus

- A pinned URLTest group whose `now` differs from `fixed` (pinned member down): the check/active highlight follows `now`, the pin follows `fixed`. Pinned in Task 10 (`a_pinned_node_carries_a_pin` uses `fixed`, not `now`) and Task 8 (`fixed={name === currentGroup.fixed}`).
- Group and node names with `/`, `?`, `#`, spaces, CJK, emoji, `:` through the DELETE path and the tray unfix id. Pinned in Task 6 (fixture `GROUP = "group/日本 ?#"`) and Task 10 (`unfix_item_ids_round_trip`).
- Clash-rs (no `fixed` field): no restore button/item and nodes of URLTest/Fallback stay unselectable. Pinned in Task 10 (`TrayGroup::of` copies `clear_fixed: false`) and Task 7/8 conditions reading `capabilities`.
- A pinned group whose members include provider-owned nodes or a failing member: provider is passed, failures become 0 samples without aborting. Pinned in Task 9 tests.
- A selection or clear-pin rejected by the core: the snapshot stays, the error surfaces (dialog on the page, `console.error` in the tray menu, log in the native tray). Pinned in Task 6 (`a_rejected_unpin_rereads_the_core_and_keeps_the_snapshot`) and Task 7 (`selectProxy` rejection test).

---

## Phase 1 — `fix/proxies-unification-followups`

The branch already exists with the spec commit `docs(proxies): specify pinned selection and unification follow-ups`; this plan is committed on top as `docs(proxies): plan pinned selection and unification follow-ups`.

### Task 1: Cover pinned Selector/Fallback capabilities and GLOBAL outside Global mode (P1)

**Files:**

- Modify: `backend/tauri/src/core/clash/proxies.rs` (test `capabilities_follow_the_type_and_the_fixed_field`)
- Modify: `backend/tauri/src/core/tray/proxies.rs` (test `global_mode_adds_a_global_entry_rule_mode_does_not`)

**Interfaces:** none (tests only; behavior already exists).

- [ ] **Step 1: Add the two capability rows**

In `capabilities_follow_the_type_and_the_fixed_field`, make the `cases` array:

```rust
        let cases = [
            ("Selector", None, (true, false)),
            ("Selector", Some(""), (true, false)),
            ("Selector", Some("a"), (true, false)),
            ("URLTest", None, (false, false)),
            ("URLTest", Some(""), (true, true)),
            ("URLTest", Some("a"), (true, true)),
            ("Fallback", None, (false, false)),
            ("Fallback", Some(""), (true, true)),
            ("Fallback", Some("a"), (true, true)),
            ("LoadBalance", None, (false, false)),
            ("Relay", None, (false, false)),
            ("Smart", None, (false, false)),
            ("Weighted", Some(""), (false, false)),
        ];
```

- [ ] **Step 2: Assert GLOBAL is absent in Rule and Script modes**

In `global_mode_adds_a_global_entry_rule_mode_does_not`, replace the three `rule_mode` lines (from `let rule_mode = ...` through its last `assert_eq!`) with:

```rust
        for mode in [Mode::Rule, Mode::Script] {
            let tray = to_tray_proxies(mode, &proxies);
            assert!(!tray.contains_key("GLOBAL"), "{mode:?}");
            assert!(!tray.contains_key("global"), "{mode:?}");
            assert!(tray.contains_key("GroupA"), "{mode:?}");
            assert_eq!(tray["GroupA"].all, vec!["node-a".to_owned()]);
        }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::clash::proxies core::tray::proxies` (run the two filters separately if the harness accepts only one).
Expected: PASS (these pin existing behavior).

- [ ] **Step 4: Commit**

```bash
git add backend/tauri/src/core/clash/proxies.rs backend/tauri/src/core/tray/proxies.rs
git commit -m "test(proxies): cover pinned Selector and Fallback, and GLOBAL outside Global mode" -m "The capability matrix had no row for a Selector or Fallback group that
reports a pinned member, and the tray test checked only that the
lowercase \"global\" alias stays out of Rule mode, which the real GLOBAL
key could still violate. Both now pin the intended behavior."
```

### Task 2: Never show negative subscription usage (P2)

**Files:**

- Modify: `frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx`
- Test: `frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`

**Interfaces:** `useProxiesSubscription(data)` keeps returning `{ progress, total, used, hasSubscriptionInfo }`.

- [ ] **Step 1: Write the failing test** (append to the test file)

```ts
test('negative counters never produce negative usage', async () => {
  const { result } = await renderHook(() =>
    useProxiesSubscription(
      provider({ Upload: -10, Download: 30, Total: -1, Expire: 0 }),
    ),
  )

  expect(result.current).toEqual({
    progress: 0,
    total: 0,
    used: 30,
    hasSubscriptionInfo: true,
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm exec vitest run frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`
Expected: FAIL (`used` is 20, `total` is -1).

- [ ] **Step 3: Implement**

In `use-proxies-subscription.tsx`, add below `clampPercentage`:

```ts
// Providers may send garbage negative counters; usage never goes below zero.
const nonNegative = (value: number) => Math.max(0, value)
```

and change the two assignments inside `if (hasSubscriptionInfo)`:

```ts
total = nonNegative(subscriptionInfo.Total)

used =
  nonNegative(subscriptionInfo.Download) + nonNegative(subscriptionInfo.Upload)
```

- [ ] **Step 4: Run the test file**

Run: `pnpm exec vitest run frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx`
Expected: PASS (all three tests).

- [ ] **Step 5: Commit**

```bash
git add 'frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx' frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx
git commit -m "fix(providers): never show negative subscription usage" -m "Subscription counters now arrive as the core's signed values instead of
failing the whole provider read, so a provider that reports a negative
upload, download, or total showed a negative used amount on the
providers page. Each counter is clamped to zero before use."
```

### Task 3: Give mocked delay samples a time (P3)

**Files:**

- Modify: `frontend/nyanpasu/tests/providers-refresh.browser.test.tsx`

- [ ] **Step 1: Fix the mock**

Replace `history: [{ delay: providerFetches }],` with:

```ts
              history: [
                { time: '2026-09-30T00:00:00Z', delay: providerFetches },
              ],
```

- [ ] **Step 2: Run the test**

Run: `pnpm exec vitest run frontend/nyanpasu/tests/providers-refresh.browser.test.tsx`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add frontend/nyanpasu/tests/providers-refresh.browser.test.tsx
git commit -m "test(providers): give mocked delay samples a time" -m "DelayHistory requires a time; the mock omitted it, so the fixture did not
match the shape the core returns."
```

### Task 4: Copy only the provider nodes a group references (P4)

**Files:**

- Modify: `backend/tauri/src/core/clash/proxies.rs` (`provider_proxy_map` and its use in `Proxies::from_responses`; tests module)

**Interfaces:** `Proxies::from_responses` signature and output unchanged.

- [ ] **Step 1: Write the regression test first** (add to the tests module)

```rust
    /// A node listed by several providers takes the last one, a provider
    /// with an unsupported vehicle contributes nothing, and only members a
    /// group references are added.
    #[test]
    fn a_node_in_several_providers_takes_the_last_provider() {
        let providers = IndexMap::from([
            provider("first", "HTTP", vec![item("shared", "Vmess", None, None)]),
            provider("second", "File", vec![item("shared", "Trojan", None, None)]),
            provider(
                "compat",
                "Compatible",
                vec![item("compat-only", "Vless", None, None)],
            ),
            provider("extra", "Inline", vec![item("unreferenced", "Vless", None, None)]),
        ]);
        let group = item("G", "Selector", Some(vec!["shared", "compat-only"]), None);
        let result = Proxies::from_responses(records(&[group]), &providers, None).unwrap();

        let shared = &result.nodes[&name("shared")];
        assert_eq!(shared.proxy_type, "Trojan");
        assert_eq!(shared.provider.as_deref(), Some("second"));
        assert_eq!(result.nodes[&name("compat-only")].proxy_type, "Unknown");
        assert!(!result.nodes.contains_key(&name("unreferenced")));
    }
```

- [ ] **Step 2: Run it against the current code**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::clash::proxies::tests::a_node_in_several_providers_takes_the_last_provider`
Expected: PASS (it pins the semantics the refactor must keep).

- [ ] **Step 3: Replace the eager map with a borrowed index**

Replace `provider_proxy_map` with:

```rust
/// Every proxy of an HTTP, File, or Inline provider by name, with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies. The index borrows, so
/// only the nodes a group references are ever copied.
fn provider_proxy_index(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<&ProxyName, (&ProviderName, &Proxy)> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            proxies.insert(&proxy.name, (provider, proxy));
        }
    }
    proxies
}
```

and in `Proxies::from_responses` replace the block from `let provider_proxies = provider_proxy_map(providers);` through the closure's `nodes.insert(...)` with:

```rust
        let provider_proxies = provider_proxy_index(providers);
        let mut convert = |record: Proxy| {
            for name in record.all.iter().flatten() {
                if !nodes.contains_key(name) {
                    let node = match provider_proxies.get(name) {
                        Some((provider, proxy)) => {
                            let mut node = Proxy::clone(proxy);
                            node.provider = Some(provider.as_str().to_owned());
                            node
                        }
                        None => unknown_proxy(name),
                    };
                    nodes.insert(name.clone(), node);
                }
            }
            ProxyGroup::from_record(&record)
        };
```

- [ ] **Step 4: Run the module tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::clash::proxies` and `... --lib core::proxies`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/clash/proxies.rs
git commit -m "perf(proxies): copy only the provider nodes a group references" -m "Every proxy snapshot cloned every node of every provider into a lookup
map, although only group members missing from /proxies are ever taken
from it. The lookup now borrows the provider records and clones a node
only when a group references it; the last provider listing a name still
wins."
```

### Task 5: Phase 1 verification, review loop, and PR

Orchestrator task (not delegated as one implementer brief).

- [ ] **Step 1: Branch-wide checks**

Run, all from the repo root:

```bash
pnpm lint:rustfmt
pnpm lint:clippy
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::
pnpm typecheck
pnpm exec vitest run frontend/nyanpasu/tests/proxies-subscription.browser.test.tsx frontend/nyanpasu/tests/providers-refresh.browser.test.tsx
pnpm lint:prettier
pnpm lint:oxlint
deno task lint:architecture-ledger
```

Expected: all pass except the known pre-existing failure.

- [ ] **Step 2: `/ccg:review` loop** on `main...fix/proxies-unification-followups`. Fix every Critical/High finding by folding the fix into the commit it corrects (`git rebase -i` is unavailable; use `git commit --fixup <sha>` followed by `GIT_SEQUENCE_EDITOR=: git rebase --autosquash -i main`). Re-review until no Critical/High remains.

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin fix/proxies-unification-followups
gh pr create --draft --base main --head fix/proxies-unification-followups --title "fix(proxies): close proxies API unification follow-ups" --body-file <scratchpad>/pr1.md
```

The body lists the four fixes, the checks run, and that the #5623/#5624 native menu GUI check (macOS, Linux, Windows) is still pending.

---

## Phase 2 — `feat/proxies-pinned-selection`

Create the branch from the reviewed Phase 1 head: `git switch -c feat/proxies-pinned-selection`.

### Task 6: Clear a group's pinned selection (B1)

**Files:**

- Modify: `backend/tauri/src/core/actor_v2/api.rs` (add `clear_proxy_selection` after `select_proxy`)
- Modify: `backend/tauri/src/core/proxies.rs` (message, shared route-change path, client method, tests)
- Modify: `backend/tauri/src/client/clash_api.rs` (add `clear_proxy_fixed` after `select_proxy`)
- Modify: `backend/tauri/src/ipc.rs` (add `clear_proxy_fixed` after `select_proxy`)
- Modify: `backend/tauri/src/specta_export.rs` (register `ipc::clear_proxy_fixed` right after `ipc::select_proxy`)
- Regenerate: `frontend/rpc/src/rpc-bindings.ts`, `frontend/query/src/query-bindings.ts` (and `frontend/rpc/src/tauri-bindings.ts` if it changes)

**Interfaces:**

- Produces (Rust): `ApiClient::clear_proxy_selection(&self, group: &ProxyName) -> Result<(), ApiError>`; `ProxiesClient::clear_fixed(&self, group: String, strategy: ProxyChangeBreakMode) -> Result<MutationOutcome<()>>`; `NyanpasuClient::clear_proxy_fixed(&self, group: String) -> Result<MutationOutcome<()>>`.
- Produces (RPC): command `clear_proxy_fixed { group: string }` returning `MutationOutcome<null>`; generated `commands.clearProxyFixed(group)` and `api.mutations.clearProxyFixed` (mutation key `['clearProxyFixed']`, input `[group]`).

- [ ] **Step 1: Make the test fixture model a pinnable group**

In `core/proxies.rs` tests:

- add `fixed: Mutex<String>,` to `Fixture`;
- in `proxies()`, change the `(GROUP, "Selector")` tuple to `(GROUP, "URLTest")` and after the `now` line add:
  ```rust
          proxies[GROUP]["fixed"] = serde_json::json!(f.fixed.lock().unwrap().clone());
  ```
- in `select()`, after `*f.selected.lock().unwrap() = NODE.into();` add `*f.fixed.lock().unwrap() = NODE.into();` (mihomo pins a URLTest group on selection);
- add the handler:
  ```rust
      async fn clear(HttpState(f): HttpState<Arc<Fixture>>, Path(group): Path<String>) -> StatusCode {
          assert_eq!(group, GROUP);
          f.calls.lock().unwrap().push("clear");
          if f.fail_mutation.load(Ordering::SeqCst) {
              return StatusCode::SERVICE_UNAVAILABLE;
          }
          f.fixed.lock().unwrap().clear();
          StatusCode::NO_CONTENT
      }
  ```
- route it: `.route("/proxies/{group}", put(select).delete(clear))`.

- [ ] **Step 2: Write the failing tests** (add to the tests module)

```rust
    #[tokio::test]
    async fn clearing_a_pin_follows_the_break_strategy_then_refreshes() {
        let (client, _core, _, fixture, server) = setup().await;
        client
            .select(GROUP.into(), NODE.into(), ProxyChangeBreakMode::Off)
            .await
            .unwrap();
        assert_eq!(
            client.snapshot().groups[0].fixed.as_ref().map(ProxyName::as_str),
            Some(NODE)
        );
        fixture.calls.lock().unwrap().clear();
        client
            .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
            .await
            .unwrap();
        assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "close", "read"]);
        assert_eq!(client.snapshot().groups[0].fixed, None);
        fixture.calls.lock().unwrap().clear();
        client
            .clear_fixed(GROUP.into(), ProxyChangeBreakMode::Off)
            .await
            .unwrap();
        assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "read"]);
        server.abort();
    }
    #[tokio::test]
    async fn clearing_a_pin_closes_only_the_group_chains() {
        let (client, _core, _, fixture, server) = setup().await;
        let outcome = client
            .clear_fixed(GROUP.into(), ProxyChangeBreakMode::ProxyGroup)
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        assert_eq!(
            *fixture.closed.lock().unwrap(),
            [uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(3)]
        );
        assert_eq!(
            *fixture.calls.lock().unwrap(),
            ["clear", "connections", "close", "close", "read"]
        );
        server.abort();
    }
    #[tokio::test]
    async fn a_rejected_unpin_rereads_the_core_and_keeps_the_snapshot() {
        let (client, _core, _, fixture, server) = setup().await;
        client.get(false).await.unwrap();
        fixture.fail_mutation.store(true, Ordering::SeqCst);
        fixture.calls.lock().unwrap().clear();
        let error = client
            .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("clear_proxy_selection"),
            "{error:#}"
        );
        assert_eq!(*fixture.calls.lock().unwrap(), ["clear", "read"]);
        assert!(!client.snapshot().groups.is_empty());
        server.abort();
    }
    #[tokio::test]
    async fn an_unpin_whose_interruption_fails_is_degraded_not_failed() {
        let (client, _core, _, fixture, server) = setup().await;
        fixture.fail_close.store(true, Ordering::SeqCst);
        let outcome = client
            .clear_fixed(GROUP.into(), ProxyChangeBreakMode::All)
            .await
            .unwrap();
        assert_eq!(outcome.degradations().len(), 1);
        assert!(matches!(
            outcome.degradations()[0].reason,
            crate::client::runtime::DegradationReason::ProxyInterruptionFailed { .. }
        ));
        assert!(
            outcome.degradations()[0]
                .message
                .starts_with("pinned selection cleared, but"),
            "{}",
            outcome.degradations()[0].message
        );
        assert!(!client.snapshot().nodes.is_empty());
        server.abort();
    }
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::proxies`
Expected: compile error (`clear_fixed` not found).

- [ ] **Step 4: Add the API wrapper** (`core/actor_v2/api.rs`, after `select_proxy`)

```rust
    pub async fn clear_proxy_selection(&self, group: &ProxyName) -> Result<(), ApiError> {
        self.execute(self.client.clear_proxy_selection(group)).await
    }
```

- [ ] **Step 5: Add the message and share the route-change path** (`core/proxies.rs`)

- Import `ProxyName`: `use clash_api::{IndexMap, ProviderName, ProxyName, ProxyProvider};`
- Add to `enum Message`, after `Select`:
  ```rust
      ClearFixed {
          group: String,
          strategy: ProxyChangeBreakMode,
          reply: RpcReplyPort<Result<MutationOutcome<()>>>,
      },
  ```
- Add above `struct ProxiesActor;`:
  ```rust
  /// A user's change to which member carries a group's traffic.
  enum RouteChange {
      Select { name: String },
      ClearFixed,
  }

  impl RouteChange {
      /// What a degradation message reports as done.
      fn done(&self) -> &'static str {
          match self {
              Self::Select { .. } => "proxy selected",
              Self::ClearFixed => "pinned selection cleared",
          }
      }
  }
  ```
- Rename `State::select` to `change_route` with this signature and body (the comments and steps are the existing `select` body, with the mutation and the two messages generalized):
  ```rust
      async fn change_route(
          &mut self,
          actor: &ActorRef<Message>,
          group: String,
          change: RouteChange,
          strategy: ProxyChangeBreakMode,
      ) -> Result<MutationOutcome<()>> {
          // The published snapshot stays until the core answers: clearing it first would hand every
          // subscriber an empty proxy list, and a rejected change would leave it empty.
          let api = match self.core.api_client().await {
              Ok(api) => api,
              Err(error) => {
                  self.clear();
                  return Err(error.into());
              }
          };
          let group_name = ProxyName::from(group.clone());
          let applied = match &change {
              RouteChange::Select { name } => {
                  api.select_proxy(&group_name, &name.clone().into()).await
              }
              RouteChange::ClearFixed => api.clear_proxy_selection(&group_name).await,
          }
          .map_err(anyhow::Error::from);
          self.reread_after_rejection(actor, api.clone(), applied)
              .await?;
          // Keep every follow-up on the change's revocable source capability.
          let interruption =
              match super::connections::ConnectionScope::for_proxy_change(strategy, group) {
                  Some(scope) => super::connections::interrupt_connections(&api, &scope).await,
                  None => Ok(()),
              };
          let mut degradations = Vec::new();
          if let Err(error) = interruption {
              degradations.push(Degradation {
                  phase: DegradationPhase::SystemEffect,
                  reason: DegradationReason::ProxyInterruptionFailed {
                      cause: InterruptFailure::from(&error),
                  },
                  message: format!(
                      "{}, but source-instance connection interruption failed: {error}",
                      change.done()
                  ),
                  retryable: false,
              });
          }
          if let Err(error) = self.refresh(actor, api).await {
              degradations.push(Degradation {
                  phase: DegradationPhase::UiEffect,
                  reason: DegradationReason::ProxyCacheRefreshFailed,
                  message: format!("{}, but cache refresh failed: {error}", change.done()),
                  retryable: true,
              });
          }
          Ok(MutationOutcome::from_parts((), degradations))
      }
  ```
- In `handle`, shutdown branch: add `Message::ClearFixed { reply, .. } => { let _ = reply.send(Err(anyhow::anyhow!("proxy owner is shutting down"))); }`.
- In `handle`, replace the `Select` arm call with `state.change_route(&actor, group, RouteChange::Select { name }, strategy).await` and add:
  ```rust
              Message::ClearFixed {
                  group,
                  strategy,
                  reply,
              } => {
                  if reply.is_closed() {
                      return Ok(());
                  }
                  let _ = reply.send(
                      state
                          .change_route(&actor, group, RouteChange::ClearFixed, strategy)
                          .await,
                  );
              }
  ```
- Add to `impl ProxiesClient`, after `select`:

  ```rust
      pub async fn clear_fixed(
          &self,
          group: String,
          strategy: ProxyChangeBreakMode,
      ) -> Result<MutationOutcome<()>> {
          self.call(|reply| Message::ClearFixed {
              group,
              strategy,
              reply,
          })
          .await
      }
  ```

- [ ] **Step 6: Run the actor tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::proxies`
Expected: PASS (new and existing tests).

- [ ] **Step 7: Expose it through the facade and RPC**

`client/clash_api.rs`, after `select_proxy`:

```rust
    pub async fn clear_proxy_fixed(
        &self,
        group: String,
    ) -> Result<super::runtime::MutationOutcome<()>> {
        let strategy = self
            .get_clash_config()
            .await?
            .break_connection
            .on_proxy_change;
        self.inner.proxies.clear_fixed(group, strategy).await
    }
```

`ipc.rs`, after `select_proxy`:

```rust
#[nyanpasu_macro::rpc(http)]
#[tauri::command]
#[specta::specta]
pub async fn clear_proxy_fixed(
    client: State<'_, NyanpasuClient>,
    group: String,
) -> Result<crate::client::runtime::MutationOutcome<()>> {
    Ok(client.clear_proxy_fixed(group).await?)
}
```

`specta_export.rs`: add `ipc::clear_proxy_fixed,` on the line after `ipc::select_proxy,`.

- [ ] **Step 8: Regenerate bindings and verify**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`
Then: `grep -n "clearProxyFixed\|clear_proxy_fixed" frontend/rpc/src/rpc-bindings.ts frontend/query/src/query-bindings.ts`
Expected: a `clearProxyFixed` command and a `clearProxyFixed: mutationOptions({ mutationKey: ['clearProxyFixed'], ...` entry under `mutations`.
Also run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib specta_export` and `pnpm typecheck`. Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add backend/tauri/src/core/actor_v2/api.rs backend/tauri/src/core/proxies.rs backend/tauri/src/client/clash_api.rs backend/tauri/src/ipc.rs backend/tauri/src/specta_export.rs frontend/rpc/src/rpc-bindings.ts frontend/query/src/query-bindings.ts
# add frontend/rpc/src/tauri-bindings.ts too if `git status` shows it modified
git commit -m "feat(proxies): clear a group's pinned selection" -m "Selecting a member of a URLTest or Fallback group pins it, and nothing
could return the group to automatic selection. The proxy actor now sends
DELETE /proxies/{group} for a ClearFixed message and runs the same
follow-up as a selection: a rejected request re-reads the core, the
break-on-proxy-change setting decides which connections close, and the
snapshot is refreshed, with failures after the core accepted reported as
degradations. clear_proxy_fixed exposes it on both transports, like
select_proxy."
```

### Task 7: Select only where the core accepts it and report failures (B2)

**Files:**

- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx`
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx`
- Modify: `frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx`
- Modify: `frontend/nyanpasu/messages/{en,ko,ru,zh-cn,zh-tw}.json`
- Test: `frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx` (create)

**Interfaces:**

- Produces: `ProxyNodeButton` prop `selectable: boolean` (required). Task 8 adds `fixed`.
- Produces: message `proxies_select_failed_message({ group, name })`.

- [ ] **Step 1: Add the i18n key** right after `proxies_group_delay_test_pending_title` in each file:

| File         | Line to add                                                                        |
| ------------ | ---------------------------------------------------------------------------------- |
| `en.json`    | `"proxies_select_failed_message": "Failed to select {name} in {group}",`           |
| `zh-cn.json` | `"proxies_select_failed_message": "无法在 {group} 中选择 {name}",`                 |
| `zh-tw.json` | `"proxies_select_failed_message": "無法在 {group} 中選擇 {name}",`                 |
| `ko.json`    | `"proxies_select_failed_message": "{group}에서 {name}을(를) 선택하지 못했습니다",` |
| `ru.json`    | `"proxies_select_failed_message": "Не удалось выбрать {name} в {group}",`          |

Then regenerate Paraglide: `pnpm web:build` (expected: build succeeds; `frontend/nyanpasu/src/paraglide/messages/` gains the key).

- [ ] **Step 2: Write the failing component test** (`frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`)

```tsx
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProxyNodeButton from '@/pages/(main)/main/proxies/group/_modules/proxy-node-button'
import type { ClashProxiesQueryProxyItem } from '@nyanpasu/query'

const proxy: ClashProxiesQueryProxyItem = {
  name: 'node-a',
  type: 'Vless',
  udp: true,
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
}

async function renderButton(selectable: boolean) {
  const onSelect = vi.fn(async () => {})
  const onDelayTest = vi.fn(async () => {})
  const screen = await render(
    <BlockTaskProvider>
      <TooltipProvider>
        <ProxyNodeButton
          proxy={proxy}
          selectable={selectable}
          onSelect={onSelect}
          onDelayTest={onDelayTest}
        />
      </TooltipProvider>
    </BlockTaskProvider>,
  )
  return { screen, onSelect, onDelayTest }
}

test('a node of a selectable group is selected on click', async () => {
  const { screen, onSelect } = await renderButton(true)
  await screen.getByText('node-a').click()
  await expect.poll(() => onSelect).toHaveBeenCalledWith(proxy)
})

test('a node of a group the core selects on its own ignores clicks but still tests', async () => {
  const { screen, onSelect, onDelayTest } = await renderButton(false)
  const card = screen.container.querySelector('[data-selectable="false"]')
  expect(card).not.toBeNull()
  await screen.getByText('node-a').click()
  // The card is the button; the latency control is the `asChild` span that
  // wraps the bolt icon (no history yet, so no delay chip).
  const delayControl = card!.querySelector('svg')!.parentElement!
  delayControl.click()
  await expect.poll(() => onDelayTest).toHaveBeenCalledWith(proxy)
  expect(onSelect).not.toHaveBeenCalled()
})
```

If the type for `ClashProxiesQueryProxyItem` requires more or fewer fields than listed, match the generated `Proxy_Serialize` type exactly (it is the source of truth). Keep the assertion that `onSelect` was never called.

- [ ] **Step 3: Run to verify it fails**

Run: `pnpm exec vitest run frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`
Expected: FAIL (type error or no `[data-selectable]` element).

- [ ] **Step 4: Implement the gating in `ProxyNodeButton`**

- Add `selectable: boolean` to the props type (next to `proxy`) and destructure it.
- Change the select handler:
  ```tsx
  const handleSelectProxy = useLockFn(async () => {
    // The core picks this group's member on its own.
    if (!selectable) {
      return
    }

    await onSelect(proxy)
  })
  ```
- On the outer `<Button variant="fab" ...>` add `data-selectable={String(selectable)}` and append to its `cn(...)` list:

  ```tsx
            'data-[selectable=false]:cursor-default',
            'data-[selectable=false]:hover:before:bg-transparent',
  ```

- [ ] **Step 5: Wire the main page** (`proxies/group/$name.tsx`)

- Import `message` from `@/utils/notification` and `m` from `@/paraglide/messages`.
- Replace `handleSelectProxy` with:
  ```tsx
  const handleSelectProxy = useCallback(
    async (proxy: ClashProxiesQueryProxyItem) => {
      if (!groupName) {
        return
      }

      try {
        await selectProxy(groupName, proxy.name)
      } catch (error) {
        message(
          m.proxies_select_failed_message({
            group: groupName,
            name: proxy.name,
          }),
          { kind: 'error', error },
        )
      }
    },
    [groupName, selectProxy],
  )
  ```
- Derive `const selectable = currentGroup?.capabilities.select ?? false` next to `groupName`, and pass `selectable={selectable}` to `<ProxyNodeButton>`.

- [ ] **Step 6: Wire the WebView tray menu** (`(tray-menu)/tray-menu/proxies/group/$name.tsx`)

- `ProxyButton` gains `selectable: boolean`:
  ```tsx
  const ProxyButton = ({
    proxy,
    selectable,
    onSelect,
  }: {
    proxy: ClashProxiesQueryProxyItem
    selectable: boolean
    onSelect: (proxy: ClashProxiesQueryProxyItem) => Promise<void>
  }) => {
    ...
    const handleClick = useLockFn(async () => {
      // The core picks this group's member on its own.
      if (!selectable) {
        return
      }

      await onSelect(proxy)
    })

    return (
      <ActionButton
        className="w-full data-[selectable=false]:cursor-default"
        data-selectable={String(selectable)}
        // A click that selects nothing must not close the menu.
        disableClose={!selectable}
        onClick={handleClick}
      >
  ```
- `handleSelectProxy` logs instead of throwing:
  ```tsx
  const handleSelectProxy = async (proxy: ClashProxiesQueryProxyItem) => {
    if (!currentGroup) {
      return
    }

    try {
      await selectProxy(currentGroup.name, proxy.name)
    } catch (error) {
      // A dialog would take focus and dismiss the tray menu; the frontend
      // error reporter records this in the application log.
      console.error('[tray-menu] failed to select proxy', error)
    }
  }
  ```
- Pass `selectable={currentGroup?.capabilities.select ?? false}` to `<ProxyButton>`.

- [ ] **Step 7: Run checks**

Run: `pnpm exec vitest run frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`, `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add 'frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx' 'frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx' 'frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx' frontend/nyanpasu/messages/en.json frontend/nyanpasu/messages/ko.json frontend/nyanpasu/messages/ru.json frontend/nyanpasu/messages/zh-cn.json frontend/nyanpasu/messages/zh-tw.json frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx
git commit -m "feat(proxies): select only where the core accepts it and report failures" -m "The page and the WebView tray menu sent a selection for every clicked
node, although the core rejects selecting LoadBalance groups and, on
Clash-rs, URLTest and Fallback groups; the failure was then dropped. Both
now follow the group's select capability from the proxy snapshot, as the
native tray does, while a node's latency test stays available. A rejected
selection opens an error dialog on the page; the tray menu logs it, since
a dialog would take focus and dismiss the menu."
```

### Task 8: Show pinned members and restore automatic selection (B3)

Depends on Task 6 (bindings) and Task 7 (`selectable` prop).

**Files:**

- Modify: `frontend/query/src/ipc/use-clash-proxies.ts` (add `clearProxyFixed`)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx` (add `fixed`)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx` (pass `fixed`, header chip and restore button)
- Modify: `frontend/nyanpasu/src/components/proxies/group-summary.tsx` (pin icon)
- Modify: `frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx` (pin icon, restore button)
- Modify: `frontend/nyanpasu/messages/{en,ko,ru,zh-cn,zh-tw}.json`
- Test: `frontend/query/tests/proxy-clear-fixed.browser.test.tsx` (create); extend `frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`

**Interfaces:**

- Consumes: `api.mutations.clearProxyFixed` (Task 6), `ProxyNodeButton.selectable` (Task 7).
- Produces: `useClashProxies().clearProxyFixed(group: string): Promise<void>`; `ProxyNodeButton` prop `fixed: boolean` (required); messages `proxies_group_fixed_label()`, `proxies_group_clear_fixed_button()`, `proxies_clear_fixed_failed_message({ group })`.

- [ ] **Step 1: Add i18n keys** right after the `proxies_select_failed_message` line from Task 7, in this order:

| File         | Lines                                                                                                                                                                                                  |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `en.json`    | `"proxies_group_fixed_label": "Pinned",` `"proxies_group_clear_fixed_button": "Restore Auto",` `"proxies_clear_fixed_failed_message": "Failed to restore automatic selection for {group}",`            |
| `zh-cn.json` | `"proxies_group_fixed_label": "已固定",` `"proxies_group_clear_fixed_button": "恢复自动",` `"proxies_clear_fixed_failed_message": "无法恢复 {group} 的自动选择",`                                      |
| `zh-tw.json` | `"proxies_group_fixed_label": "已固定",` `"proxies_group_clear_fixed_button": "恢復自動",` `"proxies_clear_fixed_failed_message": "無法恢復 {group} 的自動選擇",`                                      |
| `ko.json`    | `"proxies_group_fixed_label": "고정됨",` `"proxies_group_clear_fixed_button": "자동으로 복원",` `"proxies_clear_fixed_failed_message": "{group}의 자동 선택을 복원하지 못했습니다",`                   |
| `ru.json`    | `"proxies_group_fixed_label": "Закреплено",` `"proxies_group_clear_fixed_button": "Вернуть авто",` `"proxies_clear_fixed_failed_message": "Не удалось восстановить автоматический выбор для {group}",` |

Regenerate Paraglide with `pnpm web:build`.

- [ ] **Step 2: Write the failing hook test** (`frontend/query/tests/proxy-clear-fixed.browser.test.tsx`)

```tsx
import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { Proxies_Serialize, ProxyGroup } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { useClashProxies } from '../src/ipc/use-clash-proxies'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const pinned: ProxyGroup = {
  name: 'auto',
  type: 'URLTest',
  all: ['a'],
  now: 'a',
  fixed: 'a',
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: true },
}

test('clearing a pin calls the mutation, then refetches the proxies', async ({
  onTestFinished,
}) => {
  let snapshot: Proxies_Serialize = {
    global: null,
    groups: [pinned],
    nodes: {},
  }
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => snapshot,
    clear_proxy_fixed: async () => {
      snapshot = { ...snapshot, groups: [{ ...pinned, fixed: null }] }
      return {
        status: 'committed',
        value: null,
        commits: [],
        notifications_pending: false,
      }
    },
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)

  await hook.act(async () => {
    await hook.result.current.clearProxyFixed('auto')
  })

  expect(testRpc.invoke).toHaveBeenCalledWith('clear_proxy_fixed', {
    group: 'auto',
  })
  await expect
    .poll(() => hook.result.current.proxies.data?.groups[0].fixed)
    .toBeNull()
})
```

If the test RPC passes mutation parameters under a different shape, match what `testRpc.invoke` receives for `select_proxy` (check `query/tests/` for an existing mutation assertion) and assert that.

- [ ] **Step 3: Run to verify it fails**

Run: `pnpm exec vitest run frontend/query/tests/proxy-clear-fixed.browser.test.tsx`
Expected: FAIL (`clearProxyFixed` is not a function).

- [ ] **Step 4: Implement `clearProxyFixed`** in `use-clash-proxies.ts`

Beside `selectProxyCommand`:

```ts
const clearProxyFixedCommand = api.mutations.clearProxyFixed
```

After the `mutateSelectProxy` mutation:

```ts
const { mutateAsync: mutateClearProxyFixed } = useMutation({
  mutationKey: clearProxyFixedCommand.mutationKey,
  mutationFn: async (group: string) =>
    unwrapResult(await invokeMutation(clearProxyFixedCommand, [group])),
})
```

After `selectProxy`:

```ts
const clearProxyFixed = useCallback(
  async (group: string) => {
    await mutateClearProxyFixed(group)
    await queryClient.refetchQueries({
      queryKey: api.queries.getProxies().queryKey,
      exact: true,
    })
  },
  [mutateClearProxyFixed, queryClient],
)
```

Return it: `return { proxies, selectProxy, clearProxyFixed, updateProxiesDelay, updateGroupDelay }`.

Run: `pnpm exec vitest run frontend/query/tests/proxy-clear-fixed.browser.test.tsx` — Expected: PASS.

- [ ] **Step 5: Mark the pinned node card**

In `ProxyNodeButton`: import `KeepRounded from '~icons/material-symbols/keep-rounded'` and `m from '@/paraglide/messages'`; add required prop `fixed: boolean`; replace the first content row with:

```tsx
<div className="flex w-full items-center justify-between gap-2 px-2">
  <div className="truncate text-sm font-medium">{proxy.name}</div>

  {fixed && (
    <span
      className="text-primary shrink-0"
      title={m.proxies_group_fixed_label()}
      data-slot="proxy-node-fixed-icon"
    >
      <KeepRounded className="size-4" />
    </span>
  )}
  {/* TODO: takes up too much space and needs to be redesigned */}
  {/* <DelayHistoryBar history={proxy.history ?? []} /> */}
</div>
```

Extend `proxy-node-button.browser.test.tsx`: give `renderButton` a second parameter `fixed = false` passed as `fixed={fixed}`, and add:

```tsx
test('only the pinned member shows the pin', async () => {
  const pinned = await renderButton(true, true)
  expect(
    pinned.screen.container.querySelector(
      '[data-slot="proxy-node-fixed-icon"]',
    ),
  ).not.toBeNull()
  pinned.screen.unmount()

  const other = await renderButton(true, false)
  expect(
    other.screen.container.querySelector('[data-slot="proxy-node-fixed-icon"]'),
  ).toBeNull()
})
```

- [ ] **Step 6: Main page header and node wiring** (`proxies/group/$name.tsx`)

- Imports: `KeepRounded from '~icons/material-symbols/keep-rounded'`, `KeepOffRounded from '~icons/material-symbols/keep-off-rounded'`, `useLockFn` from `@nyanpasu/hooks`.
- Destructure `clearProxyFixed` from `useClashProxies()`.
- Below `handleSelectProxy`:
  ```tsx
  const handleClearFixed = useLockFn(async () => {
    if (!groupName) {
      return
    }

    try {
      await clearProxyFixed(groupName)
    } catch (error) {
      message(m.proxies_clear_fixed_failed_message({ group: groupName }), {
        kind: 'error',
        error,
      })
    }
  })
  ```
- Pass `fixed={name === currentGroup?.fixed}` to `<ProxyNodeButton>`.
- In `GroupHeader`, inside the name column (`<div className="flex max-w-full min-w-0 flex-col gap-1">`), after the name `<div>`:
  ```tsx
  {
    currentGroup?.fixed && (
      <div
        className="text-on-surface-variant flex min-w-0 items-center gap-1 text-xs"
        title={currentGroup.fixed}
        data-slot="proxies-group-fixed"
      >
        <KeepRounded className="size-3.5 shrink-0" />
        <span className="shrink-0">{m.proxies_group_fixed_label()}</span>
        <span className="truncate">{currentGroup.fixed}</span>
      </div>
    )
  }
  ```
- After `<div className="flex-1" />` and before the radar button:

  ```tsx
  {
    currentGroup?.fixed && currentGroup.capabilities.clearFixed && (
      <Button
        variant="stroked"
        className="flex h-8 shrink-0 items-center gap-1 px-3 text-sm"
        onClick={handleClearFixed}
        data-slot="proxies-group-clear-fixed-button"
      >
        <KeepOffRounded className="size-4" />
        <span>{m.proxies_group_clear_fixed_button()}</span>
      </Button>
    )
  }
  ```

- [ ] **Step 7: Sidebar summary pin** (`components/proxies/group-summary.tsx`)

Import `KeepRounded from '~icons/material-symbols/keep-rounded'`; inside the `group.now && (<>...</>)` fragment, before the `now` marquee `div`:

```tsx
{
  group.fixed && (
    <span
      className="shrink-0"
      title={group.fixed}
      data-slot="group-summary-fixed-icon"
    >
      <KeepRounded className="size-3.5" />
    </span>
  )
}
```

- [ ] **Step 8: WebView tray menu** (`(tray-menu)/tray-menu/proxies/group/$name.tsx`)

- Imports: `KeepRounded`, `KeepOffRounded` (as above), `m` already imported.
- `ProxyButton` gains `fixed: boolean`; after the `TextMarquee`:
  ```tsx
  {
    fixed && (
      <span
        className="text-primary shrink-0"
        title={m.proxies_group_fixed_label()}
        data-slot="tray-menu-proxy-fixed-icon"
      >
        <KeepRounded className="size-4" />
      </span>
    )
  }
  ```
  and the call site passes `fixed={name === currentGroup?.fixed}`.
- Add beside `DelayTestButton`:
  ```tsx
  const ClearFixedButton = ({ group }: { group: string }) => {
    const { clearProxyFixed } = useClashProxies()

    const handleClick = async () => {
      try {
        await clearProxyFixed(group)
      } catch (error) {
        // A dialog would take focus and dismiss the tray menu; the frontend
        // error reporter records this in the application log.
        console.error(
          '[tray-menu] failed to restore automatic selection',
          error,
        )
      }
    }

    return (
      <ActionButton
        className="w-10 shrink-0 justify-center backdrop-blur-lg"
        title={m.proxies_group_clear_fixed_button()}
        disableClose
        onClick={handleClick}
        data-slot="tray-menu-clear-fixed-button"
      >
        <KeepOffRounded />
      </ActionButton>
    )
  }
  ```
- In the sticky header, before `<DelayTestButton />`:

  ```tsx
  {
    currentGroup?.fixed && currentGroup.capabilities.clearFixed && (
      <ClearFixedButton group={currentGroup.name} />
    )
  }
  ```

- [ ] **Step 9: Run checks**

Run: `pnpm exec vitest run frontend/query/tests/proxy-clear-fixed.browser.test.tsx frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx`, `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add frontend/query/src/ipc/use-clash-proxies.ts 'frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx' 'frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx' frontend/nyanpasu/src/components/proxies/group-summary.tsx 'frontend/nyanpasu/src/pages/(tray-menu)/tray-menu/proxies/group/$name.tsx' frontend/nyanpasu/messages/en.json frontend/nyanpasu/messages/ko.json frontend/nyanpasu/messages/ru.json frontend/nyanpasu/messages/zh-cn.json frontend/nyanpasu/messages/zh-tw.json frontend/query/tests/proxy-clear-fixed.browser.test.tsx frontend/nyanpasu/tests/proxy-node-button.browser.test.tsx
git commit -m "feat(proxies): show pinned members and restore automatic selection" -m "Selecting a member of a URLTest or Fallback group pins it, but no surface
showed the pin, so a user could not tell why the group stopped switching
on its own. The node card, the group header, the sidebar summary, and
the WebView tray menu now mark the pinned member, and a group whose core
can clear the pin offers to restore automatic selection."
```

### Task 9: Keep a group's pin through a latency test (B4)

**Files:**

- Modify: `frontend/query/src/ipc/use-clash-proxies.ts` (`updateGroupDelay.mutationFn`, local helper, constant)
- Test: `frontend/query/tests/proxy-delay-history.browser.test.tsx`

**Interfaces:** `updateGroupDelay.mutateAsync([group, options?])` unchanged; returns `Record<string, number>` for both paths.

- [ ] **Step 1: Write the failing tests** (append to `proxy-delay-history.browser.test.tsx`)

```tsx
function makePinnedSnapshot(): Proxies_Serialize {
  return {
    global: group('GLOBAL'),
    groups: [
      {
        ...group('group'),
        type: 'URLTest',
        fixed: 'tested',
        capabilities: { select: true, clearFixed: true },
      },
    ],
    nodes: {
      tested: node('tested'),
      other: { ...node('other'), provider: 'sub' },
    },
  }
}

async function testPinnedGroup(
  delay: (params?: Record<string, unknown>) => Promise<unknown>,
  onTestFinished: TestContext['onTestFinished'],
) {
  const snapshot = makePinnedSnapshot()
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  const testRpc = createTestRpc({
    get_proxies: async () => snapshot,
    clash_api_get_proxy_delay: delay,
  })
  const hook = await renderHook(() => useClashProxies(), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllTimers()
    vi.useRealTimers()
  })
  await expect.poll(() => hook.result.current.proxies.isSuccess).toBe(true)
  const queryKey = createQueryBindings(testRpc.rpc).queries.getProxies()
    .queryKey
  const data = () => client.getQueryData<ClashProxiesQuery>(queryKey)!
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  await hook.act(async () => {
    await hook.result.current.updateGroupDelay.mutateAsync(['group'])
  })
  return { data, invoke: testRpc.invoke }
}

test('a pinned group tests its members one by one and never the group', async ({
  onTestFinished,
}) => {
  const { data, invoke } = await testPinnedGroup(
    async () => ({ delay: 100 }),
    onTestFinished,
  )
  expect(invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'tested',
    provider: null,
    url: null,
  })
  expect(invoke).toHaveBeenCalledWith('clash_api_get_proxy_delay', {
    name: 'other',
    provider: 'sub',
    url: null,
  })
  expect(
    invoke.mock.calls.some(
      ([method]) => method === 'clash_api_get_group_delay',
    ),
  ).toBe(false)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
  expect(data().nodes.other.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
})

test('a failing member of a pinned group records a failed sample and the rest still run', async ({
  onTestFinished,
}) => {
  const { data } = await testPinnedGroup(async (params) => {
    if (params?.name === 'tested') throw new Error('timeout')
    return { delay: 100 }
  }, onTestFinished)
  expect(data().nodes.tested.history.map(({ delay }) => delay)).toEqual([42, 0])
  expect(data().nodes.other.history.map(({ delay }) => delay)).toEqual([
    42, 100,
  ])
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `pnpm exec vitest run frontend/query/tests/proxy-delay-history.browser.test.tsx`
Expected: the two new tests FAIL (`Unexpected RPC command: clash_api_get_group_delay`); existing tests PASS.

- [ ] **Step 3: Implement**

At module level in `use-clash-proxies.ts`, after `withDelaySample`:

```ts
// How many members of a pinned group are tested at once.
const PINNED_GROUP_DELAY_CONCURRENCY = 8

// Runs `task` for every item, at most `limit` at a time.
const forEachConcurrently = async <T>(
  items: readonly T[],
  limit: number,
  task: (item: T) => Promise<void>,
) => {
  let next = 0
  const worker = async () => {
    while (next < items.length) {
      const item = items[next]
      next += 1
      await task(item)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, worker),
  )
}
```

Replace the body of `updateGroupDelay`'s `mutationFn`:

```ts
    mutationFn: async (args: [string, ClashDelayOptions?]) => {
      const [group, options] = args
      const url = options?.url ?? null
      const data = getQueryData()
      const target =
        data?.global?.name === group
          ? data.global
          : data?.groups.find((item) => item.name === group)

      // Mihomo and Meow clear a pin before testing a group, so a pinned group
      // tests its members one by one instead.
      if (!target?.fixed) {
        return (
          unwrapResult(
            await invokeQuery(api.queries.clashApiGetGroupDelay(group, url)),
          ) ?? {}
        )
      }

      const delays: Record<string, number> = {}
      await forEachConcurrently(
        target.all,
        PINNED_GROUP_DELAY_CONCURRENCY,
        async (name) => {
          try {
            const result = unwrapResult(
              await invokeQuery(
                api.queries.clashApiGetProxyDelay(
                  name,
                  data?.nodes[name]?.provider ?? null,
                  url,
                ),
              ),
            )
            delays[name] = result?.delay ?? 0
          } catch {
            // The core records a failed test as a zero sample; mirror it.
            delays[name] = 0
          }
        },
      )
      return delays
    },
```

(`getQueryData` is declared before `updateGroupDelay`; if it is declared after, move its declaration above.)

- [ ] **Step 4: Run tests and checks**

Run: `pnpm exec vitest run frontend/query/tests/proxy-delay-history.browser.test.tsx`, `pnpm typecheck`, `pnpm lint:oxlint`, `pnpm lint:prettier`
Expected: PASS (all tests in the file).

- [ ] **Step 5: Commit**

```bash
git add frontend/query/src/ipc/use-clash-proxies.ts frontend/query/tests/proxy-delay-history.browser.test.tsx
git commit -m "fix(proxies): keep a group's pin through a latency test" -m "Mihomo and Meow clear a URLTest or Fallback pin before testing the
group (getGroupDelay calls ForceSet(\"\")), so testing a pinned group from
the page or the WebView tray menu silently returned it to automatic
selection. A pinned group now tests its members one by one through the
single-node endpoint, eight at a time, recording a failed member as a
zero sample; an unpinned group still uses the group endpoint."
```

### Task 10: Mark pinned nodes and restore automatic selection in the native tray (B5)

Depends on Task 6 (`NyanpasuClient::clear_proxy_fixed`).

**Files:**

- Modify: `backend/tauri/src/core/tray/proxies.rs`
- Modify: `backend/tauri/locales/{en,ko,ru,zh-cn,zh-tw}.json`

**Interfaces:**

- Consumes: `NyanpasuClient::clear_proxy_fixed(group: String) -> Result<MutationOutcome<()>>`; `ProxyGroup.fixed: Option<ProxyName>`; `ProxyGroup.capabilities.clear_fixed: bool`.
- Produces: `TrayGroup { now, all, selectable, fixed: Option<String>, clear_fixed: bool }`; `unfix_item_id`, `parse_unfix_item_id`, `node_item_text`.

- [ ] **Step 1: Add the tray string** right after `"select_proxy"` inside `"tray"`:

| File         | Line                                                             |
| ------------ | ---------------------------------------------------------------- |
| `en.json`    | `"restore_auto_selection": "Restore Automatic Selection",`       |
| `zh-cn.json` | `"restore_auto_selection": "恢复自动选择",`                      |
| `zh-tw.json` | `"restore_auto_selection": "恢復自動選擇",`                      |
| `ko.json`    | `"restore_auto_selection": "자동 선택 복원",`                    |
| `ru.json`    | `"restore_auto_selection": "Восстановить автоматический выбор",` |

- [ ] **Step 2: Write the failing tests** (tests module of `core/tray/proxies.rs`)

Update the `selecting` helper's `TrayGroup` literal with `fixed: None, clear_fixed: false,`. Add `unfix_item_id("Proxy").as_str(),` to the id list in `other_menu_ids_are_not_node_items`. Add:

```rust
    #[test]
    fn unfix_item_ids_round_trip() {
        for group in ["Proxy", "a:b", "sp ace", "\u{1F680}", "", "proxy_node:[\"x\"]"] {
            assert_eq!(parse_unfix_item_id(&unfix_item_id(group)), Some(group));
        }
    }

    #[test]
    fn node_and_unfix_ids_never_parse_as_each_other() {
        assert_eq!(parse_unfix_item_id(&node_item_id("G", "n")), None);
        assert_eq!(parse_node_item_id(&unfix_item_id("G")), None);
        assert_eq!(parse_unfix_item_id(&group_menu_id("G")), None);
    }

    /// The pin follows `fixed`, not `now`: a pinned URLTest member that is
    /// down keeps its pin while the check moves to the member in use.
    #[test]
    fn a_pinned_node_carries_a_pin() {
        assert_eq!(node_item_text("a", Some("a")), "a 📌");
        assert_eq!(node_item_text("b", Some("a")), "b");
        assert_eq!(node_item_text("a", None), "a");
    }

    #[test]
    fn tray_groups_carry_the_pin_and_whether_it_can_be_cleared() {
        let mut pinned = group("Auto", &["node-a", "node-b"], "node-b");
        pinned.kind = ProxyGroupKind::UrlTest;
        pinned.fixed = Some("node-a".into());
        pinned.capabilities = ProxyGroupCapabilities {
            select: true,
            clear_fixed: true,
        };
        let tray = TrayGroup::of(&pinned);
        assert_eq!(tray.fixed.as_deref(), Some("node-a"));
        assert_eq!(tray.now.as_deref(), Some("node-b"));
        assert!(tray.clear_fixed);

        // Clash-rs reports no pin: nothing to clear.
        let automatic = group("Fallback", &["node-a"], "node-a");
        let tray = TrayGroup::of(&automatic);
        assert_eq!(tray.fixed, None);
        assert!(!tray.clear_fixed);
    }

    /// The pin is part of the item text and the restore item exists only
    /// for clearable groups, so either change needs a rebuild.
    #[test]
    fn a_pin_change_needs_a_rebuild() {
        let open = selecting(Some("a"));
        let mut pinned = selecting(Some("a"));
        pinned["Proxy"].fixed = Some("a".to_owned());
        assert_eq!(diff_proxies(&open, &pinned), TrayUpdateType::Full);
        let mut clearable = selecting(Some("a"));
        clearable["Proxy"].clear_fixed = true;
        assert_eq!(diff_proxies(&open, &clearable), TrayUpdateType::Full);
    }
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`
Expected: compile errors (`unfix_item_id`, `node_item_text`, `fixed` field missing).

- [ ] **Step 4: Implement**

Next to the existing id helpers:

```rust
const UNFIX_ITEM_ID_PREFIX: &str = "proxy_unfix:";

/// The id of a group's "restore automatic selection" item. The group name is
/// the whole rest of the id, so it needs no escaping.
fn unfix_item_id(group: &str) -> String {
    format!("{UNFIX_ITEM_ID_PREFIX}{group}")
}

/// The group an unfix item id names; `None` for any other menu item.
fn parse_unfix_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(UNFIX_ITEM_ID_PREFIX)
}

/// A node's menu text; the pinned member carries a pin.
fn node_item_text(node: &str, fixed: Option<&str>) -> String {
    if fixed == Some(node) {
        format!("{node} 📌")
    } else {
        node.to_owned()
    }
}
```

`TrayGroup` gains:

```rust
    /// The member a user pinned, marked in the menu.
    pub(super) fixed: Option<String>,
    /// Whether the menu offers to return the group to automatic selection.
    pub(super) clear_fixed: bool,
```

and `TrayGroup::of` fills them:

```rust
            fixed: group.fixed.as_ref().map(|name| name.as_str().to_owned()),
            clear_fixed: group.capabilities.clear_fixed,
```

In `diff_proxies`, extend the structural check:

```rust
        // check if the length of all list, the selectability, or the pin is different
        if item.all.len() != old_item.all.len()
            || item.selectable != old_item.selectable
            || item.fixed != old_item.fixed
            || item.clear_fixed != old_item.clear_fixed
        {
            return TrayUpdateType::Full;
        }
```

In `platform_impl`, import `node_item_text` and `unfix_item_id` from `super`, then in `generate_group_selector`, after the `if group.all.is_empty() { ... }` early return and before the node loop:

```rust
        if group.clear_fixed {
            group_menu = group_menu
                .item(
                    &MenuItemBuilder::with_id(
                        unfix_item_id(group_name),
                        t!("tray.restore_auto_selection"),
                    )
                    .enabled(group.fixed.is_some())
                    .build(app_handle)?,
                )
                .separator();
        }
```

and build each node item with `CheckMenuItemBuilder::new(node_item_text(item, group.fixed.as_deref()))` instead of `CheckMenuItemBuilder::new(item.clone())`.

Add the click path. At the top of `on_system_tray_event`:

```rust
    if let Some(group) = parse_unfix_item_id(event) {
        clear_fixed(app_handle, group.to_owned());
        return;
    }
```

and, after `on_system_tray_event`:

```rust
/// Returns `group` to automatic selection. Like a selection, the tray
/// repaints once the core answered; a plain item is not flipped by the
/// platform menu, so nothing is recorded as clicked.
fn clear_fixed(app_handle: &AppHandle, group: String) {
    let client = app_handle
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        debug!("received clear pinned proxy event: {group}");
        match client.clear_proxy_fixed(group.clone()).await {
            Ok(outcome) => {
                for degradation in outcome.degradations() {
                    warn!(reason = ?degradation.reason, message = %degradation.message, "clearing the pinned proxy degraded");
                }
            }
            Err(error) => error!("clear pinned proxy failed, {group}: {error:#}"),
        }
        log_err!(Tray::request(&app_handle, TrayWork::PROXIES));
    });
}
```

If `MenuItemBuilder::with_id` is not available in this Tauri version, use `MenuItemBuilder::new(text).id(unfix_item_id(group_name))`, matching how `no_proxies` is built.

- [ ] **Step 5: Run tests and lints**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib core::tray`, `pnpm lint:clippy`, `pnpm lint:rustfmt`
Expected: PASS; no new clippy warnings in `core/tray/proxies.rs`.

- [ ] **Step 6: Commit**

```bash
git add backend/tauri/src/core/tray/proxies.rs backend/tauri/locales/en.json backend/tauri/locales/ko.json backend/tauri/locales/ru.json backend/tauri/locales/zh-cn.json backend/tauri/locales/zh-tw.json
git commit -m "feat(tray): mark pinned nodes and restore automatic selection" -m "The native tray pins a URLTest or Fallback group when a user selects one
of its members, but never showed the pin or offered to undo it. The
pinned member's item now carries a pin, and a group whose core can clear
the pin starts with a restore item, disabled while nothing is pinned.
Item lookup goes by id, so the pin in the text does not break partial
repaints; a pin change rebuilds the menu."
```

### Task 11: Phase 2 verification, review loop, and stacked PR

Orchestrator task.

- [ ] **Step 1: Branch-wide checks**

```bash
pnpm lint:rustfmt
pnpm lint:clippy
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings && git diff --exit-code frontend/rpc/src frontend/query/src/query-bindings.ts
pnpm typecheck
pnpm test:frontend
pnpm lint
deno task lint:architecture-ledger
```

Expected: all pass except the known pre-existing failure; bindings unchanged after regeneration.

- [ ] **Step 2: `/ccg:review` loop** on `fix/proxies-unification-followups...feat/proxies-pinned-selection`, folding Critical/High fixes into the commit they correct (fixup + autosquash onto `fix/proxies-unification-followups`), until no Critical/High remains.

- [ ] **Step 3: Push and open the stacked PR**

```bash
git push -u origin feat/proxies-pinned-selection
gh pr create --draft --base fix/proxies-unification-followups --head feat/proxies-pinned-selection --title "feat(proxies): gate selection on capabilities and manage pinned groups" --body-file <scratchpad>/pr2.md
```

The body summarizes B1–B5, lists the checks, and the manual GUI checks: native tray pin marker and restore item on macOS/Linux/Windows; page and WebView tray menu gating, pin, restore; pinned group latency test keeps `fixed`; the RPC on both IPC and HTTP transports.

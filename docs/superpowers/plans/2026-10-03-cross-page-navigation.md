# Cross-page Navigation (Rules / Connections / Traffic) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the rules, connections and traffic pages jump into each other with the exact same selection the source number counts, and return to the exact source state.

**Architecture:** A "return ticket" in TanStack Router history state (`returnTo`, `focus`) gives every jump an in-app way back via `history.go(delta)`. The connections page adopts the traffic page's query vocabulary (`scope`, `range`, `filters`); the backend lists closed connections and live connection ids with the same `group_key` matcher the traffic report uses, so counts agree by construction.

**Tech Stack:** Rust (`nyanpasu-traffic`, ractor `TrafficActor`, unified RPC + specta), React 19, TanStack Router 1.170 / Query / Table / Virtual, zod, Paraglide, Vitest (node + browser).

**Spec:** `docs/superpowers/specs/2026-10-03-cross-page-navigation/design.md`

## Global Constraints

- Follow `AGENTS.md` and `docs/development/*.md`; TS style per `docs/development/typescript.md` (logical blank-line groups, `data-slot` on meaningful parts, project UI components only, no direct Radix imports outside `frontend/ui`).
- Work happens in the main checkout on branch `feat/cross-page-navigation`. **Implementers do not commit**; the leader reviews and commits. Never stage `backend/Cargo.lock` (unless a dependency changed), `backend/nyanpasu-runtime`, or `.pnpm-store/`.
- i18n keys are added by Task 2 only (all five locales `frontend/nyanpasu/messages/{en,zh-cn,zh-tw,ko,ru}.json`, placed beside related keys, same order in every file). Later tasks only use them.
- Do not hand-edit generated files (`frontend/rpc/src/rpc-bindings.ts`, `frontend/query/src/query-bindings.ts`, `route-tree.gen.ts`, Paraglide output). Regenerate bindings with `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`.
- New RPCs are queries in `specta_export.rs` and `#[nyanpasu_macro::rpc(http)]` like the existing traffic queries.
- Page components stay router-agnostic where they are already tested without a router (`TrafficPage`, `TrafficToolbar`, `ActiveViewer`, `ClosedViewer`, `ConnectionDetailModal`, `StatusTabs`): router-dependent behavior enters through props/callbacks from route files.
- Route search params that are UI state use `replace: true` navigations; every in-page navigation on the three pages passes `state: keepReturn`.

## Review Focus

1. A filter that matches nothing in a large closed history: pages come back empty with a `next` cursor; the table must keep loading until a match or the end, and must not stop on the first empty page (Task 1 merge test, Task 4 browser test).
2. Returning after several in-page navigations (tab switch, chip removal, sort change) on the destination: one click must land on the source entry, not on an intermediate state (Task 2 router test).
3. A rule label shared by several rules: only the first row (the one that can match) offers jumps, and focus on return lands on that row (Task 5 test).
4. A connection that closes while its detail dialog is open or while the "All" view shows it: no duplicate row (active wins), no crash (Task 6 test).
5. Opening a filtered connections view while the traffic store is unavailable (recording disabled): closed half shows the existing unavailable message, the active half shows nothing rather than every connection (Task 4 test).

---

## Execution waves

| Wave | Tasks (parallel within a wave)                                                                                        |
| ---- | --------------------------------------------------------------------------------------------------------------------- |
| 1    | Task 1 (backend + bindings + query hooks), Task 2 (return ticket infra + i18n), Task 3 (shared traffic filter module) |
| 2    | Task 4 (connections: scope/filters/range/q, chips, return)                                                            |
| 3    | Task 5 (rules), Task 6 (connections "All" view)                                                                       |
| 4    | Task 7 (traffic), Task 8 (connections detail actions + row focus)                                                     |

---

### Task 1: Backend — list closed connections and live ids by usage filters

**Files:**

- Modify: `backend/nyanpasu-traffic/src/model.rs` (add `ClosedSelection`, `check_filters`, change `merge_closed_page`)
- Modify: `backend/nyanpasu-traffic/src/bucket.rs` (add `TrafficRange::start_ms`)
- Modify: `backend/nyanpasu-traffic/src/ports.rs` (`closed_connections` signature)
- Modify: `backend/nyanpasu-traffic/src/redb.rs` (filtered scan with budget + tests)
- Modify: `backend/nyanpasu-traffic/src/accounting.rs` (`Session::active_ids` + test)
- Modify: `backend/tauri/src/core/traffic/actor.rs`, `client.rs`, `tests.rs`
- Modify: `backend/tauri/src/client/traffic.rs` (facade + fake store in tests)
- Modify: `backend/tauri/src/ipc.rs`, `backend/tauri/src/specta_export.rs`
- Regenerate: `frontend/rpc/src/rpc-bindings.ts`, `frontend/query/src/query-bindings.ts`
- Modify: `frontend/query/src/ipc/use-traffic-closed-connections.ts`
- Create: `frontend/query/src/ipc/use-traffic-active-connection-ids.ts` (+ export in `frontend/query/src/ipc/index.ts`)
- Modify call site: `frontend/nyanpasu/src/pages/(main)/main/connections/_modules/closed-viewer.tsx` (pass the unfiltered selection)
- Modify tests: `frontend/nyanpasu/tests/connections-closed-tab.browser.test.tsx` (IPC args)

**Interfaces:**

- Produces (Rust):
  ```rust
  // model.rs
  /// Which closed connections a listing selects: closed at or after `since_ms`, satisfying every
  /// filter the way a report does.
  #[derive(Clone, Debug, Default, PartialEq, Eq)]
  pub struct ClosedSelection { pub since_ms: Option<i64>, pub filters: Vec<TrafficFilter> }
  impl ClosedSelection {
      pub fn matches(&self, conn: &ClosedConnection) -> bool;
      /// Whether no connection closed before `closed_at` can match any more.
      pub fn exhausted_at(&self, closed_at: i64) -> bool; // closed_at < since_ms
  }
  /// Rejects filters that name a dimension more than once (`TrafficQuery::check` calls this).
  pub fn check_filters(filters: &[TrafficFilter]) -> TrafficResult<()>;
  pub fn merge_closed_page(stored: ClosedPage, pending: &[ClosedConnection],
      before: Option<&ClosedCursor>, limit: usize, selection: &ClosedSelection) -> ClosedPage;
  pub(crate) const MAX_CLOSED_SCAN: usize = 20_000;
  // bucket.rs
  impl TrafficRange { /// Wall-clock ms where `start` begins; `None` for `All`.
      pub fn start_ms(self, now_ms: i64) -> Option<i64>; }
  // ports.rs
  fn closed_connections(&self, before: Option<&ClosedCursor>, limit: usize,
      selection: &ClosedSelection) -> TrafficResult<ClosedPage>;
  // accounting.rs
  impl Session { pub fn active_ids(&self, filters: &[TrafficFilter]) -> Vec<String>; } // sorted
  // core/traffic/client.rs
  pub async fn closed_connections(&self, range: TrafficRange, filters: Vec<TrafficFilter>,
      before: Option<ClosedCursor>, limit: usize) -> Result<ClosedPage>;
  pub async fn active_connection_ids(&self, filters: Vec<TrafficFilter>) -> Result<Vec<String>>;
  // client/traffic.rs (NyanpasuClient)
  pub async fn query_traffic_closed_connections(&self, range: TrafficRange,
      filters: Vec<TrafficFilter>, before: Option<ClosedCursor>, limit: usize) -> Result<ClosedPage>;
  pub async fn query_traffic_active_connection_ids(&self, filters: Vec<TrafficFilter>)
      -> Result<Vec<String>>;
  // ipc.rs
  query_traffic_closed_connections(client, range, filters, before, limit) -> Result<ClosedPage>
  query_traffic_active_connection_ids(client, filters) -> Result<Vec<String>>
  ```
- Produces (TS, `@nyanpasu/query`):
  ```ts
  export type ClosedConnectionsSelection = { range: TrafficRange; filters: TrafficFilter[] }
  export function useTrafficClosedConnections(selection: ClosedConnectionsSelection): UseInfiniteQueryResult<InfiniteData<ClosedPage>, ...>
  // queryKey: ['traffic-closed-connections', selection]
  export function useTrafficActiveConnectionIds(
    filters: TrafficFilter[],
    options?: { enabled?: boolean },
  ): UseQueryResult<string[]>
  // queryKey: ['traffic-active-connection-ids', filters]; refetchInterval 1000;
  // placeholderData: keepPreviousData; retry: false
  ```

Semantics that must hold:

- `matches`: `since_ms.is_none_or(|s| conn.closed_at >= s)` and every filter `group_key(&conn.dimensions, f.dimension) == f.value`.
- `start_ms`: `start(now).map(|b| b as i64 * bucket_ms)` with `MINUTE_MS` for `Tier::Minute`, `HOUR_MS` for `Tier::Hour` (bucket indexes are `wall_ms / bucket_ms`; confirm against `bucket()` in `bucket.rs`). A closed connection is counted by the report in the bucket of its close time, so `closed_at >= start_ms` ⇔ counted by a report over that range.
- Store scan (redb): newest first strictly before `before`; stop when `selection.exhausted_at(closed_at)` (then `next = None`); skip non-matching rows; stop after `limit` matches (`next` = last match, as today when the page is full) or after scanning `MAX_CLOSED_SCAN` rows (`next` = cursor of the last **scanned** row, even when no row matched). Unfiltered selection behaves exactly as before.
- `merge_closed_page`: filter `pending` with `selection.matches` before merging; keep the floor logic; `next = if truncated { last kept cursor } else { stored.next }`. (Today's rule `more = len > limit || stored.next.is_some()` would end paging on an empty budget-limited page.)
- Actor: `ClosedConnections(TrafficRange, Vec<TrafficFilter>, Option<ClosedCursor>, usize, reply)` validates with `check_filters`, builds `ClosedSelection { since_ms: range.start_ms(self.clock.now_ms()), filters }`, passes it to the store and the merge. New `ActiveIds(Vec<TrafficFilter>, reply)` validates and returns `self.session.active_ids(&filters)`.

- [ ] **Step 1: Write failing `nyanpasu-traffic` tests**

In `redb.rs` tests (next to `closed_connections_page_newest_first`), add tests that flush closed connections with distinct dimensions and assert:

```rust
#[test]
fn closed_connections_select_by_filters_and_since() {
    // closed a (rule X, closed_at 1_000), b (rule Y, 2_000), c (rule X, 3_000)
    // filters [Rule == X] -> [c, a], next None
    // since_ms Some(2_000), no filters -> [c, b], next None
    // since_ms Some(2_000), filters [Rule == X] -> [c], next None
}

#[test]
fn a_sparse_filter_pages_by_the_scan_budget() {
    // MAX_CLOSED_SCAN + 1 non-matching rows newer than one matching row:
    // first page: connections empty, next == Some(cursor of the MAX_CLOSED_SCAN-th scanned row)
    // second page from that cursor: [the matching row], next None
}
```

In `model.rs` tests:

```rust
#[test]
fn merge_keeps_paging_after_an_empty_budget_page() {
    // stored = ClosedPage { connections: vec![], next: Some(cursor k) }, pending empty
    // merge(..., limit 10, &ClosedSelection::default()).next == Some(cursor k)
}
#[test]
fn merge_filters_pending_connections() { /* a pending row failing the filter is left out */ }
#[test]
fn check_filters_rejects_a_repeated_dimension() { /* same message as TrafficQuery::check */ }
```

In `bucket.rs` tests: `start_ms` of `LastHour` at `now = 10 * HOUR_MS + 5 * MINUTE_MS + 7` equals `9 * HOUR_MS + 5 * MINUTE_MS`; `Last24Hours` equals `(10 - 24).max(0)…` aligned to the hour (pick a `now` large enough); `All` is `None`.
In `accounting.rs` tests: `active_ids` returns the ids of observed live connections whose dimensions match, sorted; empty filters return all.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-traffic`
Expected: compile errors / failures for the new items.

- [ ] **Step 3: Implement the `nyanpasu-traffic` changes**

`TrafficQuery::check` becomes `check_filters(&self.filters)`. Store scan sketch:

```rust
let mut scanned = 0usize;
let mut last_scanned: Option<ClosedCursor> = None;
for entry in table.range::<(u64, &str)>((Bound::Unbounded, upper)).map_err(storage)?.rev() {
    let (_, value) = entry.map_err(storage)?;
    let conn: ClosedConnection = decode(value.value())?;
    if selection.exhausted_at(conn.closed_at) {
        return Ok(ClosedPage { connections, next: None });
    }
    scanned += 1;
    let cursor = conn.cursor();
    if selection.matches(&conn) {
        connections.push(conn);
        if connections.len() == limit {
            return Ok(ClosedPage { next: Some(cursor), connections });
        }
    }
    last_scanned = Some(cursor);
    if scanned == MAX_CLOSED_SCAN {
        return Ok(ClosedPage { connections, next: last_scanned });
    }
}
Ok(ClosedPage { connections, next: None })
```

(`ClosedConnection::cursor` is private in `model.rs`; make it `pub(crate)`.) Keep `limit.clamp(1, MAX_CLOSED_PAGE)`.

- [ ] **Step 4: Run `nyanpasu-traffic` tests until they pass**

Run: `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-traffic`
Expected: all pass, including the pre-existing paging tests.

- [ ] **Step 5: Actor / client / facade / IPC**

Update the actor messages, `TrafficClient`, `NyanpasuClient` facade, both IPC commands and `specta_export.rs` (register `ipc::query_traffic_active_connection_ids` next to `ipc::query_traffic_closed_connections` in the queries list). Update every `TrafficStore` impl in tests (`core/traffic/tests.rs` wrapper store, `client/traffic.rs` disabled-recording store). Add actor tests in `core/traffic/tests.rs` next to `closed_connections_appear_before_and_after_flush`:

```rust
#[tokio::test]
async fn closed_connections_select_pending_and_stored_alike() {
    // two closed connections with different processes; one flushed, one pending;
    // filter Process == "curl" returns exactly the curl one before and after flush;
    // a range whose start_ms is after both close times returns none.
}
#[tokio::test]
async fn active_connection_ids_follow_the_filters() {
    // observe a frame with live "a" (curl) and "b" (wget);
    // filters [Process == curl] -> ["a"]; [] -> ["a", "b"];
    // a repeated dimension -> Err(InvalidRequest)
}
```

Extend `disabled_recording_reports_unavailable` in `client/traffic.rs` with the new facade call.

- [ ] **Step 6: Run backend tests and lints**

Run:

```bash
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-traffic
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib traffic
pnpm lint:clippy
pnpm lint:rustfmt
```

Expected: pass (if `rejected_source_never_dispatches` fails it is the known pre-existing failure #5532; report it, do not fix it).

- [ ] **Step 7: Regenerate bindings and update the query hooks**

Run: `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib export_typescript_bindings`
Then rewrite `use-traffic-closed-connections.ts`:

```ts
export type ClosedConnectionsSelection = {
  range: TrafficRange
  filters: TrafficFilter[]
}

/** Closed connections `selection` picks, newest first. */
export function useTrafficClosedConnections(
  selection: ClosedConnectionsSelection,
) {
  const api = useQueryApi()

  return useInfiniteQuery({
    queryKey: ['traffic-closed-connections', selection] as const,
    initialPageParam: null as ClosedCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await api.queryTrafficClosedConnections(
          selection.range,
          selection.filters,
          pageParam,
          PAGE_SIZE,
        ),
      ),
    // ...existing getNextPageParam / refetchInterval / retry unchanged
  })
}
```

Create `use-traffic-active-connection-ids.ts` (`useQuery`, key `['traffic-active-connection-ids', filters]`, `refetchInterval: 1000`, `placeholderData: keepPreviousData`, `retry: false`, `enabled: options?.enabled ?? true`). Export it from `ipc/index.ts`. In `closed-viewer.tsx`, call `useTrafficClosedConnections(UNFILTERED)` with a module constant `const UNFILTERED = { range: 'all', filters: [] } satisfies ClosedConnectionsSelection` (Task 4 replaces it). Update the IPC mock in `connections-closed-tab.browser.test.tsx` for the new argument names (`range`, `filters`, `before`, `limit`).

- [ ] **Step 8: Frontend checks**

Run: `pnpm typecheck && pnpm test:frontend connections-closed-tab`
Expected: pass. Report the diff summary; do not commit.

Leader commit: `feat(traffic): list closed connections and live ids by usage filters` (body: why — the connections page must list exactly what a report counts, so the store filters with the report's matcher and pages by a scan budget).

---

### Task 2: Return ticket infrastructure and i18n

**Files:**

- Create: `frontend/nyanpasu/src/components/router/cross-navigation.ts`
- Create: `frontend/nyanpasu/src/components/router/use-cross-navigate.ts`
- Create: `frontend/nyanpasu/src/components/router/return-button.tsx`
- Modify: `frontend/nyanpasu/src/components/router/animated-outlet.tsx` (`AnimatedOutletPreset` direction)
- Modify: `frontend/nyanpasu/messages/{en,zh-cn,zh-tw,ko,ru}.json`
- Test: `frontend/nyanpasu/tests/cross-navigation.test.ts`, `frontend/nyanpasu/tests/cross-navigation.browser.test.tsx`

**Interfaces:**

- Produces:
  ```ts
  // cross-navigation.ts
  export type CrossPage = 'rules' | 'connections' | 'traffic'
  export type ReturnTicket = {
    page: CrossPage
    /** The origin entry's href, used when that entry is no longer behind this one. */
    href: string
    /** The origin entry's `__TSR_index`. */
    index: number
  }
  declare module '@tanstack/react-router' {
    // or '@tanstack/history' — whichever typechecks; add
    interface HistoryState {
      // `@tanstack/history` as a direct dependency only if needed
      returnTo?: ReturnTicket
      /** The item the page scrolls to and highlights when it shows this entry. */
      focus?: string
    }
  }
  export type ReturnStep =
    { kind: 'go'; delta: number } | { kind: 'href'; href: string }
  export function returnStep(
    ticket: ReturnTicket,
    currentIndex: number,
  ): ReturnStep
  /** State for navigations within a page: the way back stays, a focus does not. */
  export function keepReturn(previous: ParsedHistoryState): HistoryState
  /** -1 when the history index went down (a step back), else undefined. */
  export function historyDirection(
    previousIndex: number,
    nextIndex: number,
  ): -1 | undefined
  // use-cross-navigate.ts
  export function useCrossNavigate(): (jump: {
    from: CrossPage
    /** Written into the origin entry, so returning scrolls back to it. */
    originFocus?: string
    /** Written into the new entry: what the target page scrolls to. */
    targetFocus?: string
    /** Typed TanStack navigate options for the target (`to`, `search`). */
    to: NavigateOptions
  }) => Promise<void>
  /** The page's focus for this entry, read once when the page mounts. */
  export function useEntryFocus(): string | undefined
  // return-button.tsx
  export function ReturnButton(props: {
    className?: string
  }): JSX.Element | null
  ```
- i18n keys (add all here; later tasks use them):
  - `navigation_return_to`: en "Back to {page}", zh-cn "返回{page}", zh-tw "返回{page}", ko "{page}(으)로 돌아가기", ru "Назад: {page}" — place near `navbar_label_*`.
  - `rules_view_connections`: en "Show these connections", zh-cn "查看这些连接" — after `rules_column_total`.
  - `rules_view_usage`: en "Show usage", zh-cn "查看用量" — after `rules_view_connections`.
  - `connections_tab_all`: en "All", zh-cn "全部" — before `connections_tab_active`.
  - `connections_column_status`: en "Status", zh-cn "状态" — after `connections_column_closed_time`.
  - `connections_locate_rule`: en "Show rule", zh-cn "定位规则" — after `connections_view_details`.
  - `connections_view_rule_usage`: en "Rule usage", zh-cn "查看规则用量" — after `connections_locate_rule`.
  - `traffic_view_connections`: en "Show connections", zh-cn "查看连接" — after `traffic_filter_clear_all`.
    (zh-tw/ko/ru: translate accordingly.)

Behavior:

- `returnStep`: `delta = ticket.index - currentIndex`; `delta < 0` → `{ kind: 'go', delta }`, else `{ kind: 'href', href: ticket.href }`.
- `useCrossNavigate`: if `originFocus` is set, first `router.navigate({ to: '.', search: true, replace: true, resetScroll: false, state: (prev) => ({ ...prev, focus: originFocus }) })` (verify in the browser test that `__TSR_index` is unchanged); then read `router.history.location` (`href`, `state.__TSR_index`) and `router.navigate({ ...to, state: { returnTo: { page: from, href, index }, focus: targetFocus } })`.
- `useEntryFocus`: `const focus = useLocation({ select: (l) => l.state.focus })`; return `useState(focus)[0]`.
- `ReturnButton`: reads `returnTo` with `useLocation`; returns `null` without a ticket. Renders `Button` (`@nyanpasu/ui/button`) with `ArrowBackIosNewRounded` (`~icons/material-symbols/arrow-back-ios-new-rounded`) + label `m.navigation_return_to({ page })` where page is `m.navbar_label_rules()` / `m.navbar_label_connections()` / `m.topology_title()`. `data-slot="return-button"`, `h-10 shrink-0 rounded-full`; label inside `<span className="@max-2xl:sr-only">` so a narrow container shows the icon only; wrapped in `Tooltip` with the same text. Click: `returnStep(ticket, router.history.location.state.__TSR_index)` → `router.history.go(delta)` or `router.navigate({ href })`.
- `AnimatedOutletPreset`: track `useRouterState({ select: (s) => s.location.state.__TSR_index })` in a ref updated on every change; when the pathname changes and `historyDirection(prevIndex, index) === -1`, use direction `-1`; otherwise keep the current ancestor logic.

- [ ] **Step 1: Write failing unit tests** (`tests/cross-navigation.test.ts`)

```ts
test('a ticket behind the current entry goes back by the gap', () => {
  expect(
    returnStep({ page: 'rules', href: '/main/rules', index: 3 }, 6),
  ).toEqual({ kind: 'go', delta: -3 })
})
test('a ticket not behind the current entry navigates to its href', () => {
  expect(
    returnStep({ page: 'rules', href: '/main/rules?q=x', index: 6 }, 6),
  ).toEqual({ kind: 'href', href: '/main/rules?q=x' })
})
test('in-page navigations keep the way back and drop the focus', () => {
  const ticket = { page: 'traffic' as const, href: '/main/topology', index: 1 }
  expect(keepReturn({ __TSR_index: 4, returnTo: ticket, focus: 'x' })).toEqual({
    returnTo: ticket,
  })
})
test('only a lower history index is a step back', () => {
  expect(historyDirection(5, 2)).toBe(-1)
  expect(historyDirection(2, 3)).toBeUndefined()
  expect(historyDirection(3, 3)).toBeUndefined()
})
```

- [ ] **Step 2: Run** `pnpm test:frontend cross-navigation` — expect FAIL (module missing).
- [ ] **Step 3: Implement `cross-navigation.ts`**, run again — PASS.
- [ ] **Step 4: Write the failing router browser test** (`tests/cross-navigation.browser.test.tsx`): build a tiny route tree (`createRootRoute`, routes `/a` and `/b` with zod search `{ n?: number }`) and `createRouter({ routeTree, history: createMemoryHistory({ initialEntries: ['/a'] }) })`; route `/a` renders a button calling `crossNavigate({ from: 'rules', originFocus: 'row-7', to: { to: '/b' } })` and shows `useEntryFocus()`; route `/b` renders `<ReturnButton />` and two buttons that `navigate({ search: { n }, state: keepReturn })` twice (pushes). Assert: after the jump the return button reads the rules label; after the two in-page pushes the button is still there; clicking it lands on `/a` with `location.state.focus === 'row-7'` and the remounted `/a` shows `row-7`; `/a` alone (no ticket) renders no return button. Wrap in `TooltipProvider`.
- [ ] **Step 5: Implement `use-cross-navigate.ts`, `return-button.tsx`, the outlet direction and the i18n keys**; run `pnpm test:frontend cross-navigation` — PASS.
- [ ] **Step 6: Checks** — `pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier`. Report; do not commit.

Leader commit: `feat(frontend): offer a way back after jumping between pages`.

---

### Task 3: Shared traffic filter module

**Files:**

- Create: `frontend/nyanpasu/src/pages/(main)/main/_modules/traffic-filters.ts`
- Move: `topology/_modules/filter-chip.tsx` → `_modules/filter-chip.tsx`; `topology/_modules/usage-label.ts` → `_modules/usage-label.ts` (update every importer; keep `data-slot="traffic-filter-chip"`)
- Create: `frontend/nyanpasu/src/pages/(main)/main/_modules/use-usage-label-of.ts`
- Modify: `topology/_modules/search.ts`, `traffic-page.tsx`, `traffic-toolbar.tsx`, `ranking-card.tsx`, `ranking-modal.tsx`, `ranking-row.tsx`, `stat-cards.tsx`, `geography-view.tsx`; `rules/_modules/use-rule-stats.ts`, `rules/index.tsx` (import `ruleLabel` from the new module)
- Modify tests: `frontend/nyanpasu/tests/traffic-search.test.ts` and any test importing moved modules

**Interfaces:**

- Produces (`_modules/traffic-filters.ts`, moved from `search.ts` without behavior change):
  ```ts
  export const RANGES: readonly TrafficRange[] // same values/order
  export const DIMENSIONS: readonly Dimension[] // same values/order
  export const searchFilterSchema = z.object({
    d: z.enum(DIMENSIONS),
    v: z.string(),
  })
  export type SearchFilter = z.infer<typeof searchFilterSchema>
  export function setFilter(
    filters: SearchFilter[],
    dimension: Dimension,
    value: string,
  ): SearchFilter[]
  export function toggleFilter(
    filters: SearchFilter[],
    dimension: Dimension,
    value: string,
  ): SearchFilter[]
  /** The URL's short filters as the backend's filters. */
  export function toTrafficFilters(
    filters: readonly SearchFilter[],
  ): TrafficFilter[]
  /** Moved from `traffic-toolbar.tsx` (the localized range names). */
  export const rangeName: (range: TrafficRange) => string
  /** Moved from `rules/_modules/use-rule-stats.ts` (update its importers). */
  export const ruleLabel: (type: string, payload: string) => string
  ```
  `topology/_modules/search.ts` imports these (no re-exports); `trafficSearchSchema.filters` becomes `z.array(searchFilterSchema).default([])`; `toQuery` uses `toTrafficFilters`.
- Produces (`_modules/use-usage-label-of.ts`): `export function useUsageLabelOf(): (dimension: Dimension, key: string) => UsageLabel` — the `profileNames` + `usageLabel` logic currently inside `TrafficPage`, which now calls this hook.

- [ ] **Step 1:** Move code; update imports (callers import from the new path; no compatibility re-export from `search.ts`).
- [ ] **Step 2:** Add a unit test in `traffic-search.test.ts`: `toTrafficFilters([{ d: 'rule', v: 'Match' }])` equals `[{ dimension: 'rule', value: 'Match' }]`; adjust existing imports.
- [ ] **Step 3:** Run `pnpm test:frontend traffic && pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier` — PASS, behavior unchanged.

Leader commit: `refactor(traffic): share filter parsing and labels beyond the traffic page`.

---

### Task 4: Connections — traffic query vocabulary, filtered rows, chips, way back

Depends on Tasks 1–3.

**Files:**

- Modify: `connections/route.tsx` (search schema; sidebar `Link` keeps return state)
- Modify: `connections/index.tsx`
- Modify: `connections/_modules/status-tabs.tsx` (scope tabs; still two segments here, `all` arrives in Task 6)
- Create: `connections/_modules/use-connection-rows.ts` (row hooks shared by viewers)
- Modify: `connections/_modules/active-viewer.tsx`, `closed-viewer.tsx`, `mock-connections.ts`
- Create: `connections/_modules/connections-filters.tsx` (chips + clear all)
- Tests: update `connections-status-tabs.browser.test.tsx`, `connections-closed-tab.browser.test.tsx`, `connections-detail.browser.test.tsx`, `connection-mock.test.ts`; add `tests/connections-filters.browser.test.tsx`

(All paths under `frontend/nyanpasu/src/pages/(main)/main/`.)

**Interfaces:**

- Consumes: Task 1 hooks, Task 2 `ReturnButton`/`keepReturn`, Task 3 `searchFilterSchema`/`RANGES`/`toTrafficFilters`/`useUsageLabelOf`/`FilterChip`/`dimensionName`.
- Produces:
  ```ts
  // route.tsx
  validateSearch: z.object({
    proxy: z.string().optional().nullable(),
    scope: z.enum(['active', 'closed']).optional(), // Task 6 adds 'all'
    range: z.enum(RANGES).optional(),
    filters: z.array(searchFilterSchema).optional(),
    q: z.string().optional(),
  })
  // status-tabs.tsx
  export type ConnectionsScope = 'active' | 'closed' // Task 6: TrafficScope
  export function StatusTabs(props: {
    value: ConnectionsScope
    onValueChange(v: ConnectionsScope): void
    activeCount?: number
    closedCount?: number
  })
  export default function ConnectionsStatusTabs(props: {
    value
    onValueChange
    selection: ConnectionsSelection
  })
  // use-connection-rows.ts
  export type ConnectionsSelection = {
    range?: TrafficRange
    filters: SearchFilter[]
  }
  export const isFiltered = (s: ConnectionsSelection) =>
    s.filters.length > 0 || s.range !== undefined
  export function useActiveConnectionRows(args: {
    search: string
    proxy?: string | null
    filters: SearchFilter[]
  }): {
    connections: ConnectionRow[] /* unfiltered by search/proxy, filtered by `filters` */
    rows: ConnectionRow[] /* after proxy + search */
    loading: boolean
  }
  export function useClosedConnectionRows(args: {
    search: string
    proxy?: string | null
    selection: ConnectionsSelection
  }): {
    rows: ClosedConnection[]
    error: unknown
    hasNextPage: boolean
    onEndReached: () => void
  }
  // viewers gain props: ActiveViewer { filters: SearchFilter[] }, ClosedViewer { selection: ConnectionsSelection }
  ```

Behavior:

- `scope` defaults to `'active'` at the read site; the tabs call `navigate({ search: (p) => ({ ...p, scope: next }), state: keepReturn })` (push, like today).
- Active rows with `filters` non-empty: `useTrafficActiveConnectionIds(toTrafficFilters(filters))`; keep only stream rows whose id is in the set; while the ids are not loaded (or the query errored), show no rows (never all rows). In mock mode (`useMockConnectionsNow() !== null`), filter mock rows with `groupKey` (import from `topology/_modules/mock-traffic.ts`) over `mockActiveDimensions(conn)` — new export in `mock-connections.ts` that builds `Dimensions` from a mock Clash row the same way `mockClosedConnections` does.
- Closed rows: `useTrafficClosedConnections({ range: selection.range ?? 'all', filters: toTrafficFilters(selection.filters) })`. Mock mode: filter `mockClosedConnections(now)` by `closed_at >= now - rangeSpan` (only when `range` is set; reuse the hour/minute spans, approximate is fine for dev mocks) and `groupKey`. The search/proxy filtering and the `searchTexts` cache move into the hooks unchanged.
- Counts (`ConnectionsStatusTabs`): unfiltered → as today. Filtered → active = `ids.length` (mock: filtered mock length); closed = `useTrafficReport({ query: { range: selection.range ?? 'all', scope: 'closed', filters }, metric: 'connections', rankings: [], ranking_limit: 1, topology: null }).data?.total.connections` (mock: filtered mock length).
- Toolbar (`data-slot="connections-toolbar"`) becomes an `@container` with: `ReturnButton`, tabs, filter chips (only when filters/range exist), search input, column settings, close-all. Wide: one row with chips scrolling horizontally in `flex-1` and the search input `w-56 @4xl:w-72`; without chips the search keeps `flex-1`. Narrow (`@max-3xl`): chips move to their own row (`order-last basis-full`), as the traffic toolbar does. Chips: one per filter, `${dimensionName(d)}: ${labelOf(d, v).text}` with `title`, plus a range chip `${m.traffic_range_label()}: ${rangeName(range)}` (`rangeName` from Task 3), plus "clear all" (`m.traffic_filter_clear_all()`) that removes filters and range. Removing navigates with `replace: true, state: keepReturn`.
- `q`: the input keeps local state initialised from `q`; a 300 ms debounce (`useDebounce` from `@uidotdev/usehooks`) writes `q` (`undefined` when empty) with `replace: true, state: keepReturn`.
- Sidebar `Item` `Link`: add `state={keepReturn}`.

- [ ] **Step 1: Failing tests.** Update `connections-status-tabs.browser.test.tsx` to the scope props. New `connections-filters.browser.test.tsx`:
  - `ActiveViewer` with `filters=[{ d: 'process', v: 'curl' }]`, IPC mock answering `query_traffic_active_connection_ids` with `['a']` and a connection-details stream mock with rows `a` and `b` → only `a` renders; with the IPC call rejecting → no rows and no crash.
  - `ClosedViewer` with a selection → the IPC mock receives `range` and `filters` (`[{ dimension: 'process', value: 'curl' }]`); pages `{connections: [], next: c1}` then `{connections: [row], next: null}` → the row renders (the empty first page does not stop loading).
  - Recording unavailable (IPC error) → the closed table shows `m.connections_closed_unavailable()`.
    Follow the IPC/stream mocking used in `connections-detail.browser.test.tsx` and `connections-closed-tab.browser.test.tsx`.
- [ ] **Step 2:** Run `pnpm test:frontend connections` — FAIL.
- [ ] **Step 3:** Implement as specified.
- [ ] **Step 4:** Run `pnpm test:frontend connections && pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier` — PASS.

Leader commit: `feat(connections): filter connections by traffic dimensions and range`.

---

### Task 5: Rules — URL state, jumps from live and total cells, focus

Depends on Tasks 2 and 4 (target search schema).

**Files:**

- Modify: `rules/route.tsx` (schema `q`, `sort`; sidebar `Link` keeps `q`/`sort` and return state)
- Modify: `rules/index.tsx` (URL-backed search/sort, jumps, focus, `ReturnButton`)
- Modify: `rules/_modules/rule-row.tsx` (`RULE_SORTS`, clickable cells, focused style)
- Test: `frontend/nyanpasu/tests/rules-row-jumps.browser.test.tsx`

**Interfaces:**

- Produces:
  ```ts
  // rule-row.tsx
  export const RULE_SORTS = ['index', 'connections', 'speed', 'total'] as const
  export type RuleSort = (typeof RULE_SORTS)[number]
  RuleRow props += { label: string; focused?: boolean;
    onViewConnections?: (label: string) => void; onViewUsage?: (label: string) => void }
  // route.tsx search: { proxy?: string | null; q?: string; sort?: Exclude<RuleSort, 'index'> }
  ```

Behavior:

- Sidebar `Link`: `search={(prev) => ({ ...prev, proxy: item })}` + `state={keepReturn}` (today it drops other params; with `q`/`sort` in the URL they must survive a proxy switch, as the component state did).
- Search input: local state from `q`, 300 ms debounce → `replace: true, state: keepReturn`. Sort: `replace: true, state: keepReturn`; `'index'` is stored as `undefined`.
- `RuleRow` (only `item.first` rows receive the callbacks): the live-connections badge is a `<button type="button">` when active and `onViewConnections` is set; the total pair is a `<button>` when the total is non-zero and `onViewUsage` is set. Both: `Tooltip` with `m.rules_view_connections()` / `m.rules_view_usage()`, `aria-label` the same, hover `bg-primary/10` rounded, `cursor-pointer`, `data-slot="rules-row-view-connections"` / `"rules-row-view-usage"`. Handlers are stable (`useCallback`) and receive the label so memoized rows do not re-render.
- Jumps (in `Viewer`, via `useCrossNavigate`):
  - connections: `{ from: 'rules', originFocus: label, to: { to: '/main/connections', search: { scope: 'active', filters: [{ d: 'rule', v: label }] } } }`
  - usage: `{ from: 'rules', originFocus: label, to: { to: '/main/topology', search: { range: 'all', scope: 'all', filters: [{ d: 'profile', v: profile ?? '' }, { d: 'rule', v: label }] } } }` — `profile` from `useCurrentProfileUid()`, the same filter `useRuleStats` totals use, so the traffic total equals the cell.
- Focus: `const focus = useEntryFocus()`; once `items` contains the first entry with `label === focus`, call `rowVirtualizer.scrollToIndex(index, { align: 'center' })` once (a ref guards repeat) and mark that row `focused` for 2 s (`bg-primary/12`, `transition-colors duration-700` fading back). A focus that is not in `items` does nothing.
- `ReturnButton` at the start of the `rules-search` bar.

- [ ] **Step 1: Failing browser test** `rules-row-jumps.browser.test.tsx`: render `RuleRow` with `live={{ connections: 3, … }}`, `total`, both callbacks → clicking the badge calls `onViewConnections('DomainSuffix,google.com')`, clicking the total calls `onViewUsage(...)`; without callbacks no button exists (`data-slot` absent); `live` with 0 connections → no button.
- [ ] **Step 2:** Run `pnpm test:frontend rules-row` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `pnpm test:frontend rule && pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier` — PASS.

Leader commit: `feat(rules): jump from a rule's live and total cells to its connections and usage`.

---

### Task 6: Connections — the "All" view

Depends on Task 4.

**Files:**

- Modify: `connections/route.tsx` (`scope: z.enum(['all', 'active', 'closed'])`)
- Modify: `connections/_modules/status-tabs.tsx` (three segments in traffic order: All, Active, Closed; `ConnectionsScope = TrafficScope`; all count = active + closed when both known)
- Create: `connections/_modules/all-viewer.tsx`
- Modify: `connections/index.tsx` (render `AllViewer` for `scope === 'all'`)
- Test: `frontend/nyanpasu/tests/connections-all-view.browser.test.tsx`; update `connections-status-tabs.browser.test.tsx`

**Interfaces:**

- Consumes: `useActiveConnectionRows`, `useClosedConnectionRows`, `activeConnectionDetail`, `closedConnectionDetail`, `ConnectionDetailModal`, `TableRow`, cells, `ConnectionsTable`.
- Produces:
  ```ts
  export type AnyConnectionRow =
    | { kind: 'active'; key: string /* `a:${id}` */; active: ConnectionRow }
    | { kind: 'closed'; key: string /* `c:${closed_at}:${id}` */; closed: ClosedConnection }
  /** Live rows first in stream order, then closed rows newest first; a closed row whose id is
   * still live is left out. */
  export function mergeConnectionRows(active: ConnectionRow[], closed: ClosedConnection[]): AnyConnectionRow[]
  export default memo(function AllViewer(props: { search: string; proxy?: string | null;
    selection: ConnectionsSelection; settingsOpen: boolean; onSettingsOpenChange(open: boolean): void }))
  ```

Behavior:

- Columns (ids reuse the existing ones so widths are shared; `settingsKey="connections-columns-all"`): `Status` (new; header `m.connections_column_status()`; a `size-2 rounded-full` dot, `bg-primary` for live, `bg-outline` for closed, with `title` = `m.connections_tab_active()` / `m.connections_tab_closed()`; size 72), `Host`, `Chains`, `Downloaded`, `Uploaded`, `DL Speed`, `UL Speed` (empty for closed), `Process`, `Rule`, `Time` (start), `Closed` (empty for live), `Source`, `Type`. Values come from the same fields the active/closed viewers use; reuse the cell components; sort functions compare numbers, empty values sort last.
- `isRowEqual`: live rows compare traffic like `sameTraffic`; closed rows are always equal; different kinds are unequal.
- Row context menu: details for both; "close connection" only for live rows. Detail modal: live rows track the live sample and fall back to the last sample after close (same logic as `ActiveViewer`; extract it into a hook in `use-connection-rows.ts` and use it in both viewers rather than copying), closed rows show the record.
- `onEndReached` loads the next closed page; empty message as the closed viewer's (unavailable / loading / empty).

- [ ] **Step 1: Failing tests:** unit-test `mergeConnectionRows` in `connections-all-view.browser.test.tsx` (or a node test if it lives in a `.ts` module): order live-then-closed-newest-first; a closed id that is live is dropped. Browser test: `AllViewer` with one live and two closed rows → three rows, the closed row has no speed text, the context menu of the closed row has no close action. Update the status-tabs test for three segments and the all count.
- [ ] **Step 2:** Run `pnpm test:frontend connections` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `pnpm test:frontend connections && pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier` — PASS.

Leader commit: `feat(connections): show live and closed connections together under All`.

---

### Task 7: Traffic — show the selected connections, way back

Depends on Tasks 2, 3, 6.

**Files:**

- Modify: `topology/route.tsx` (cross navigation; `onSearchChange` keeps return state)
- Modify: `topology/_modules/traffic-page.tsx` (props `onViewConnections?: () => void`, `toolbarStart?: ReactNode`, passed through)
- Modify: `topology/_modules/traffic-toolbar.tsx` (slot at the start; "show connections" icon button before pause)
- Test: extend `frontend/nyanpasu/tests/traffic-page.browser.test.tsx`

Behavior:

- Toolbar: `toolbarStart` renders first (the route passes `<ReturnButton />`). New icon `Button` (icon `~icons/material-symbols/lan-outline-rounded` or another existing material-symbols list/connection icon) with `Tooltip`/`aria-label` `m.traffic_view_connections()`, `data-slot="traffic-view-connections"`, rendered only when `onViewConnections` is given, placed right before the pause button.
- Route: `onViewConnections={() => crossNavigate({ from: 'traffic', to: { to: '/main/connections', search: { scope, range, filters } } })}`; `onSearchChange` navigates with `state: keepReturn`.

- [ ] **Step 1: Failing test:** in `traffic-page.browser.test.tsx`, render `TrafficPage` with `onViewConnections` spy → clicking the button (by its aria-label) calls it once; without the prop the button is absent; `toolbarStart` content renders inside `[data-slot=traffic-toolbar]`.
- [ ] **Step 2:** Run `pnpm test:frontend traffic-page` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `pnpm test:frontend traffic && pnpm typecheck && pnpm lint:oxlint && pnpm lint:prettier` — PASS.

Leader commit: `feat(traffic): open the connections the traffic page selects`.

---

### Task 8: Connections — detail actions and focus on return

Depends on Tasks 2, 5, 6.

**Files:**

- Modify: `connections/_modules/table-row.tsx` (`ConnectionDetailModal` optional actions)
- Modify: `connections/_modules/connections-table.tsx` (`focusRowId` prop: scroll + highlight)
- Modify: `active-viewer.tsx`, `closed-viewer.tsx`, `all-viewer.tsx` (pass actions and focus through)
- Modify: `connections/index.tsx` (handlers via `useCrossNavigate`, `useEntryFocus`)
- Test: extend `frontend/nyanpasu/tests/connections-detail.browser.test.tsx`; add a `ConnectionsTable` focus test

**Interfaces:**

- Produces:
  ```ts
  ConnectionDetail += { ruleLabel: string; rowId: string }  // filled by activeConnectionDetail / closedConnectionDetail
  ConnectionDetailModal props += { onLocateRule?: (detail: ConnectionDetail) => void;
    onViewRuleUsage?: (detail: ConnectionDetail) => void }
  viewer props += { onLocateRule?, onViewRuleUsage?, focusRowId?: string }
  ConnectionsTable props += { focusRowId?: string }
  ```
  `ruleLabel` uses the shared `ruleLabel(type, payload)` from `_modules/traffic-filters.ts` (Task 3). `rowId` equals the table's `getRowId` for that viewer (`id` for live, `${closed_at}:${id}` for closed, `a:`/`c:` keys in All).

Behavior:

- Modal footer: left-aligned `flat` buttons "定位规则" (`m.connections_locate_rule()`) and "查看规则用量" (`m.connections_view_rule_usage()`) when the callbacks exist; close/close-connection stay at the end.
- `index.tsx` handlers:
  - locate: `crossNavigate({ from: 'connections', originFocus: detail.rowId, targetFocus: detail.ruleLabel, to: { to: '/main/rules', search: {} } })` (empty search: no proxy/q so the rule is visible).
  - usage: `crossNavigate({ from: 'connections', originFocus: detail.rowId, to: { to: '/main/topology', search: { filters: [{ d: 'rule', v: detail.ruleLabel }] } } })`.
- `focusRowId`: `index.tsx` passes `useEntryFocus()` to the visible viewer; `ConnectionsTable` scrolls the matching row to the center once when it first appears and highlights it for 2 s (`data-focused`, same style as rules). A row that never appears does nothing.

- [ ] **Step 1: Failing tests:** detail modal with both callbacks shows both buttons and calls them with the detail (assert `ruleLabel` `'Match'` for a `Match` rule with empty payload); without callbacks no buttons. `ConnectionsTable` with `focusRowId` of the 150th row scrolls it into view (`scrollTop > 0`, row in the DOM with `data-focused`).
- [ ] **Step 2:** Run `pnpm test:frontend connections` — FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `pnpm test:frontend && pnpm typecheck && pnpm lint` — PASS.

Leader commit: `feat(connections): jump from a connection to its rule and the rule's usage`.

---

## Final verification (leader)

- [ ] `pnpm typecheck`, `pnpm lint`, `pnpm test:frontend`
- [ ] `cargo test --manifest-path backend/Cargo.toml -p nyanpasu-traffic`, `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib traffic`, `pnpm lint:clippy`, `pnpm lint:rustfmt`
- [ ] Bindings current: re-run the export and `git diff --exit-code frontend/rpc frontend/query/src/query-bindings.ts`
- [ ] Whole-branch review against the spec; report remaining limitations.

# TypeScript and React code style

Follow this guide for handwritten TS/TSX. Keep generated bindings and generated
routes under their existing generation workflows. Application calls follow the
[unified RPC rules](rpc.md); tests follow [testing and review](testing.md).
Frontend package ownership and allowed dependency directions are defined in
[Frontend packages](frontend-packages.md).

## Existing formatter and lint conventions

- Prettier uses two spaces, LF, single quotes, no semicolons, and trailing commas.
  It also sorts imports and Tailwind classes through the configured plugins.
- Keep imports at the top, avoid duplicate imports, and use the configured package
  aliases and relative paths for local modules. Keep components/types in PascalCase,
  hooks prefixed with `use`, and files in the surrounding kebab-case convention.
- Prefer `const` unless reassignment is necessary. Use strict comparisons, except
  the configured `== null`/`!= null` check for both null and undefined.
- Respect strict TypeScript checking in each package's `tsconfig.json`. Shared
  frontend packages export source through root index entries and package subpaths and typecheck with
  `noEmit`; they do not publish build output. The root `tsconfig.json` is a
  project-reference index, not a `tsc -b` build pipeline. `pnpm typecheck` checks
  each package, then the Vite/Node configuration, performance, and test projects. See
  [Frontend packages](frontend-packages.md). Use explicit
  domain types, `as const` for literal values where appropriate, and `unknown` plus
  narrowing for untrusted input. Avoid adding `any` or file-wide typecheck bypasses.
- Follow the existing Oxlint React checks: valid hooks, stable list keys, no duplicate
  props, no direct state mutation, self-closing empty components, shorthand boolean
  props, and shorthand fragments. Review dependency warnings rather than suppressing
  them to force an effect's schedule.
- `.oxlintrc.json` uses explicit rules and path overrides, not every recommended
  category. Some existing TypeScript rules are warnings; documentation does not
  silently raise their severity. Keep lint suppressions narrow and explain why.

## Group code by responsibility

Use one blank line between distinct responsibilities: query-client access, query
setup, mutation setup, derived values, effects/handlers, and the final return. Keep
related statements together; do not insert a blank line after every declaration.
Separate a return from preceding setup or computation, and attach comments to the
group they explain. Preserve compact single-purpose early-return branches.

For example, `useReleaseChannel` should keep these groups separate:

```ts
const queryClient = useQueryClient()

const query = useQuery({
  queryKey,
  queryFn: async () => unwrapResult(await rpc.getReleaseChannel()),
})

const mutation = useMutation({
  mutationFn: async (channel: ReleaseChannel) =>
    unwrapResult(await rpc.setReleaseChannel(channel)),
  onSuccess: (_, channel) => queryClient.setQueryData(queryKey, channel),
})

return { query, mutation }
```

This illustrates grouping; preserve the hook's actual lifecycle callbacks when
editing it. [use-proxy-mode.ts](../../frontend/query/src/ipc/use-proxy-mode.ts)
also demonstrates spacing between inputs, derived state, handlers, and output.
Prettier preserves useful blank lines but cannot identify these responsibilities.

## Reuse constants and environment checks

Before adding a literal, environment check, or named constant, search for an existing
definition and its callers. Inspect the owning package's constants/utilities and
the dependency's public API first. Shared immutable values belong in
`@nyanpasu/constants`; OS detection and shared OS flags live in
`@nyanpasu/platform`. Tauri
provides an `isTauri()` API.

- Reuse definitions that mean the same thing. Do not duplicate browser/Tauri
  detection in pages/hooks or create competing `isDesktop` interpretations.
- If a local constant is useful to multiple modules, consider promoting it to the
  smallest shared scope: feature module or `@nyanpasu/constants`. Pure helpers
  belong in `@nyanpasu/utils`; reusable React hooks belong in `@nyanpasu/hooks`.
  Update the related callers together. Shared packages must not import the app.
- Keep one-use implementation details local. Similar literals with different
  meanings do not need a common constant, and speculative reuse is insufficient.
- Distinguish native Tauri execution, OS identity, and viewport size. Define what
  `isDesktop` means before reusing it. Runtime-varying values need a function/hook
  or subscribed state, rather than a stale module-level boolean.
- Shared constants must be immutable and safe to import in their supported runtime.
  Avoid turning constant extraction into hidden mutable services or eager native calls.

## Name component parts with data-slot

Prefer a stable, descriptive `data-slot` on component roots and meaningful parts
that reach the DOM. It identifies structure for readers, inspection, and custom
CSS independently of changing layout classes. Use lowercase kebab-case names that
describe purpose, such as `proxy-mode-container`, `modal-content`, or `button-loading`.
Scope generic part names under the component root when writing CSS.

- Use a component name for its root and a component/part name for internal elements.
  Avoid layout-dependent names, generated IDs, translated text, and array indexes.
- Reuse slots supplied by `@nyanpasu/ui` rather than renaming them accidentally.
  Wrappers must forward `data-*` props or place the slot on their actual DOM root.
- Fragments and logic-only components have no DOM slot. Do not add a wrapper solely
  to attach one, or require a slot on every decorative element.
- Treat established slots used by custom CSS as a styling contract: preserve them
  during refactors or explicitly account for selector changes. Slots do not replace
  accessible names, semantic HTML, or React list keys.

The [system settings route](<../../frontend/nyanpasu/src/pages/(main)/main/settings/system/route.tsx>)
and its composed settings components demonstrate this convention.

## Build UI from project components

Search `@nyanpasu/ui` before implementing an interactive UI control. Its public API
uses a root `index.ts` entry (`import { Button, Card } from '@nyanpasu/ui'`)
and component subpaths for lazy loading. Compose those components in app features
and pages.
The visual result must follow Material You and the project's existing typography,
spacing, shape, motion, and semantic color tokens (`primary`, `surface`,
`on-surface`, etc.). Preserve
dark-mode, focus, disabled, loading, and keyboard behavior; avoid a parallel design system.

Use `cva()` from `class-variance-authority` for reusable class definitions, including
styles without variants. Name these definitions in camelCase with a `Variants`
suffix and call them at the use site. Do not extract class strings into
`*_CLASS` or `*_CLASSES` constants. Keep one-use layout classes inline; use `cn()`
to merge generated classes with caller overrides, not to define reusable styles.

Dashboard widgets must not contain internal scrollbars. Keep content within the
widget's allocated size with `min-h-0` and `overflow-hidden`; do not use
`overflow-auto` or `overflow-scroll` to accommodate widget content. Reduce visible
ranking rows to fit the available height and fold hidden rows into the existing
other total. Clip any remaining overflow rather than creating a scrollable area.
Apply this rule to compact, expanded, loading, empty, and error states.

If a needed component is missing, check Radix UI for a suitable primitive first.
When one exists, wrap and style it in the UI package, expose the appropriate
props and slots, and reuse that wrapper. If no primitive fits, implement a focused
accessible project component there. Plain semantic layout elements can remain local.

Oxlint enforces this boundary with `no-restricted-imports`: `radix-ui` and
`@radix-ui/*` imports belong inside `frontend/ui/`, not feature/page modules.
This prevents bypassing project styling; it cannot judge Material You fidelity or
whether a new component should have reused an existing one.

## Compose components around their responsibilities

- For compound controls, keep the meaningful parts visible in JSX and follow the
  existing project/Radix composition, such as `Select`, `SelectTrigger`,
  `SelectContent`, and `SelectItem`. Prefer this over one opaque component whose
  props describe every part of the control.
- Add a wrapper when it owns reusable behavior, accessibility, a styling contract,
  or a domain concept. Do not add layers that only rename a component, forward the
  same props, or introduce another layout element. Reuse the existing UI wrapper
  directly when it already provides the needed behavior and styling. Even when
  intermediate components have their own purpose, avoid serially forwarding the
  same state and callbacks through each layer when their descendants can read
  shared values from an appropriately scoped context.
- Keep component props focused on that component's responsibility. Do not make a
  single component switch between unrelated layouts or behaviors through a large
  collection of optional props. Compose focused components at the page or feature
  boundary instead; keep one-off composition local.
- Keep state with the nearest component that owns it. When several descendants
  need the same feature state or callbacks, provide them through a feature-scoped
  context and a named hook in the nearest shared module. Keep state and callbacks
  as direct props when they serve only one child; do not create broad application
  context for page-specific state. Keep transient interaction state, such as hover
  or dialog-open state, local to the component that uses it.

For example, compose a select from its semantic parts:

```tsx
<Select value={range} onValueChange={onRangeChange}>
  <SelectTrigger aria-label="Time range">
    <SelectValue placeholder="Choose a range" />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="last_hour">Last hour</SelectItem>
    <SelectItem value="last24_hours">Last 24 hours</SelectItem>
  </SelectContent>
</Select>
```

Use a feature-scoped context and hook when multiple descendants share state or
callbacks; expose only the values they share. Keep each page's composition and
page-specific data selection in its route.

## Organize routed pages and their modules

- A feature with distinct navigable pages must use nested file routes and a
  route outlet, rather than a large conditional page component under `_modules/`.
  The parent `route.tsx` owns the shared layout and state that must survive child
  navigation, such as filters, scope, time range, and paused updates.
- Use the project's `AnimatedOutletPreset` from
  `@/components/router/animated-outlet` as the default outlet for nested app pages.
  It supplies the shared route transitions and history direction; do not build
  page-specific animation wrappers. Use a plain `Outlet` only when the route has
  a concrete reason to omit transitions. Keep the surrounding scroll surface
  clipped (`overflow-clip`) so entering and exiting pages cannot spill into the
  sidebar or create horizontal overflow, and preserve content sizing and spacing
  inside the animated wrapper. The animated page must fill the available content
  height, even on short routes: provide a resolved height or viewport-derived
  minimum height on its parent, use a column flex layout, and let the outlet grow
  with `flex-1`. Carry this height chain through scroll-area content wrappers;
  `h-full` on a child of an auto-height parent is insufficient. Keep long content
  scrollable rather than fixing every page to the viewport height. See the
  component's JSDoc for the layout contract.
- Place a child page in its own directory with `index.tsx`, for example
  `topology/source/index.tsx`, instead of a flat `topology/source.tsx`. Place
  modules used only by that page in `topology/source/_modules/`.
- The `index.tsx` must define the named page component and own its composition,
  including data selection, loading, and empty states. Do not make it a one-line
  forwarding wrapper around a separate generic page controller. Reuse focused
  presentational components and hooks instead.
- Determine production module scope from its actual consumers. Components,
  hooks, and helpers used only by the map belong in `topology/map/_modules/`;
  those used only by connection paths belong in `topology/flow/_modules/`.
  Do not keep page-specific modules in the parent's `_modules/` or promote them
  there for hypothetical future reuse. Test files still follow the package's
  `tests/` layout described in the testing guide.
- Place shared feature modules in the nearest common ancestor's `_modules/`,
  for example `topology/_modules/traffic-context.tsx`. Do not place helper files
  alongside route files or use a leading `-` to hide them from route generation.
  The app's existing route generator ignores `_modules/`.
- Use router links for page navigation and preserve the parent's typed search
  state and cross-navigation return ticket. Keep shared query and polling owners
  in the parent so child navigation does not reset pause state or duplicate
  subscriptions. Redirect existing shared URLs when paths change.
- Regenerate the route tree through the app's existing Vite route plugin; never
  hand-edit generated route declarations.

## Organize i18n messages by feature

- Use the existing feature/component namespace for new message keys. Dashboard
  widget text, including configuration controls, uses `dashboard_widget_*`
  (for example, `dashboard_widget_proxy_shortcuts_config_orientation`). Do not create a parallel
  prefix such as `dashboard_config_*` for the same component family.
- Include the owning component's name for messages used only by that component.
  For example, messages exclusive to `core_service` carry a `core_service` segment
  under their feature namespace. Dashboard options exclusive to `ProxyShortcutsWidget`
  or `CoreShortcutsWidget` use `dashboard_widget_proxy_shortcuts_*` or
  `dashboard_widget_core_shortcuts_*`. Shared messages belong to the smallest
  common scope, such as `dashboard_widget_config_*` for shared configuration UI.
- Place new keys beside the related messages in every locale file, keeping the
  same logical groups and order across locales. Do not append unrelated new keys
  at the end of the file. Update locale sources and regenerate Paraglide output
  through its existing workflow; do not hand-edit generated message modules.

## What is enforced

| Requirement                                                                                          | Enforcement                         |
| ---------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Mechanical formatting, imports, Tailwind class ordering                                              | Prettier                            |
| Existing correctness, hooks, props, and import rules                                                 | Oxlint                              |
| Radix boundary                                                                                       | Oxlint error outside `frontend/ui/` |
| Types                                                                                                | Package TypeScript checks           |
| CSS declarations and selectors                                                                       | Existing Stylelint configuration    |
| Meaningful slots, reusable constants, logical blank lines, component reuse, design and i18n grouping | Mandatory code review               |

Run `pnpm lint:oxlint`, `pnpm lint:prettier`, and `pnpm typecheck` as appropriate.
The [Oxlint import rule](https://oxc.rs/docs/guide/usage/linter/rules/eslint/no-restricted-imports)
supports the enforced boundary. Custom JS rules are possible, but syntax checks
cannot reliably decide semantic reuse or visual design. These review requirements
remain mandatory even when all tools pass.

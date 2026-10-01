# TypeScript and React code style

Follow this guide for handwritten TS/TSX. Keep generated bindings and generated
routes under their existing generation workflows. Application calls follow the
[unified RPC rules](rpc.md); tests follow [testing and review](testing.md).

## Existing formatter and lint conventions

- Prettier uses two spaces, LF, single quotes, no semicolons, and trailing commas.
  It also sorts imports and Tailwind classes through the configured plugins.
- Keep imports at the top, avoid duplicate imports, and use the configured package
  aliases and relative paths for local modules. Keep components/types in PascalCase,
  hooks prefixed with `use`, and files in the surrounding kebab-case convention.
- Prefer `const` unless reassignment is necessary. Use strict comparisons, except
  the configured `== null`/`!= null` check for both null and undefined.
- Respect strict TypeScript checking in each package's `tsconfig.json`. Use explicit
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
editing it. [use-proxy-mode.ts](../../frontend/interface/src/ipc/use-proxy-mode.ts)
also demonstrates spacing between inputs, derived state, handlers, and output.
Prettier preserves useful blank lines but cannot identify these responsibilities.

## Reuse constants and environment checks

Before adding a literal, environment check, or named constant, search for an existing
definition and its callers. Inspect the owning package's constants/utilities and
the dependency's public API first. For example, application OS flags already live
in `frontend/nyanpasu/src/consts.ts`; Tauri provides an `isTauri()` API.

- Reuse definitions that mean the same thing. Do not duplicate browser/Tauri
  detection in pages/hooks or create competing `isDesktop` interpretations.
- If a local constant is useful to multiple modules, consider promoting it to the
  smallest shared scope: feature module, package constants, or `@nyanpasu/utils`.
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
- Reuse slots supplied by `components/ui/` rather than renaming them accidentally.
  Wrappers must forward `data-*` props or place the slot on their actual DOM root.
- Fragments and logic-only components have no DOM slot. Do not add a wrapper solely
  to attach one, or require a slot on every decorative element.
- Treat established slots used by custom CSS as a styling contract: preserve them
  during refactors or explicitly account for selector changes. Slots do not replace
  accessible names, semantic HTML, or React list keys.

The [system settings route](<../../frontend/nyanpasu/src/pages/(main)/main/settings/system/route.tsx>)
and its composed settings components demonstrate this convention.

## Build UI from project components

Search `frontend/nyanpasu/src/components/ui/` before implementing an interactive UI
control. Compose those components in features and pages. The visual result must
follow Material You and the project's existing typography, spacing, shape, motion,
and semantic color tokens (`primary`, `surface`, `on-surface`, etc.). Preserve
dark-mode, focus, disabled, loading, and keyboard behavior; avoid a parallel design system.

If a needed component is missing, check Radix UI for a suitable primitive first.
When one exists, wrap and style it in `components/ui/`, expose the appropriate
props and slots, and reuse that wrapper. If no primitive fits, implement a focused
accessible project component there. Plain semantic layout elements can remain local.

Oxlint enforces this boundary with `no-restricted-imports`: `radix-ui` and
`@radix-ui/*` imports belong inside `components/ui/`, not feature/page modules.
This prevents bypassing project styling; it cannot judge Material You fidelity or
whether a new component should have reused an existing one.

## What is enforced

| Requirement                                                                           | Enforcement                           |
| ------------------------------------------------------------------------------------- | ------------------------------------- |
| Mechanical formatting, imports, Tailwind class ordering                               | Prettier                              |
| Existing correctness, hooks, props, and import rules                                  | Oxlint                                |
| Radix boundary                                                                        | Oxlint error outside `components/ui/` |
| Types                                                                                 | Package TypeScript checks             |
| CSS declarations and selectors                                                        | Existing Stylelint configuration      |
| Meaningful slots, reusable constants, logical blank lines, component reuse and design | Mandatory code review                 |

Run `pnpm lint:oxlint`, `pnpm lint:prettier`, and `pnpm typecheck` as appropriate.
The [Oxlint import rule](https://oxc.rs/docs/guide/usage/linter/rules/eslint/no-restricted-imports)
supports the enforced boundary. Custom JS rules are possible, but syntax checks
cannot reliably decide semantic reuse or visual design. These review requirements
remain mandatory even when all tools pass.

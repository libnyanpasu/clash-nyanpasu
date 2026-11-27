# Clash Nyanpasu Web Implementation Mapping

How Material 3 Expressive and Android 17 principles map directly onto Clash Nyanpasu's frontend architecture (`docs/development/typescript.md`, `docs/development/frontend-packages.md`, `docs/development/frontend-platform.md`).

---

## 1. Package Ownership & Boundaries

| Layer                             | Owning Package                                     | Responsibilities                                                                                                                                                                                                                                                      |
| --------------------------------- | -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Dynamic Theme & Tokens**        | `@nyanpasu/theme` (`frontend/theme/`)              | Generate full M3E dynamic color schemes (`SchemeTonalSpot` / `SchemeExpressive` + `MaterialDynamicColors` including all 5 `surface-container-*`, `surface-dim`, `surface-bright`, `*-fixed*`), syntax harmonization, contrast-level support, and fallback CSS tokens. |
| **Tailwind v4 Theme & Utilities** | `frontend/nyanpasu/src/assets/styles/tailwind.css` | Expose M3E color roles, 10-step shape tokens, emphasized typography utilities, spring bezier variables, Android 17 frosted glass surface utilities, and `corner-shape: superellipse(1.25)` (keeping `rounded-full` / `rounded-none` as `round`).                      |
| **Design System Primitives**      | `@nyanpasu/ui` (`frontend/ui/src/`)                | All reusable controls wrapping `radix-ui` / `react-aria-components` / `motion/react` with `cva()`, `cn()`, M3E shape morphing, spring physics, `data-slot`, and accessible keyboard/touch semantics. **Oxlint forbids importing `radix-ui` outside `frontend/ui/`.**  |
| **Responsive Breakpoints**        | `@nyanpasu/hooks` (`frontend/hooks/`)              | Window-size-class hooks (`compact`, `medium`, `expanded`, `large`, `extraLarge`) derived from viewport width — never OS or shell guesses.                                                                                                                             |
| **Platform Facts**                | `@nyanpasu/platform` (`frontend/platform/`)        | Distinguish `surface` (`desktop`, `browser`, future mobile), `clientOs`, and `bridge` (`tauri` vs `none`). Gate window titlebars/native controls here, not on viewport width.                                                                                         |
| **App Composition & Pages**       | `@nyanpasu/nyanpasu` (`frontend/nyanpasu/`)        | Compose screens from `@nyanpasu/ui` primitives, nested TanStack Router routes, `AnimatedOutletPreset`, feature-scoped contexts, and Paraglide i18n.                                                                                                                   |

---

## 2. Token & CSS Conventions in This Repo

- **Use `cva()` (`*Variants`)** for reusable component style definitions; group long Tailwind class lists into arrays by responsibility: `[layout, typography, colors, interaction states, motion]`. Never create `*_CLASS` / `*_CLASSES` string constants.
- **Preserve `data-slot` contracts:** Every component root and meaningful internal part keeps a stable lowercase kebab-case `data-slot` (used by `pnpm run generate:data-slots` and user Custom CSS). Forward `data-*` props through wrappers.
- **Eliminate legacy surface hacks:** Replace ad-hoc `--color-mixed-background`, `--color-pane`, `--color-code`, and hardcoded `#f7f7f7` / `#0b0b0b` body backgrounds with canonical `surface`, `surface-container-lowest`, `surface-container-low`, `surface-container`, `surface-container-high`, and `surface-container-highest` tokens.
- **Android 17 frosted surface utility:** Centralize tinted frosted glass in `@nyanpasu/ui` (`glassSurfaceVariants` / `GlassSurface`) with `@media (prefers-reduced-transparency: reduce)` and `@supports not (backdrop-filter: blur(1px))` solid fallbacks. Do not scatter raw `backdrop-blur-* bg-white/*` classes in pages.
- **Motion tokens:** Export shared M3E spring presets (`M3E_MOTION.expressive.fastSpatial`, `defaultSpatial`, `slowSpatial`, `fastEffects`, `defaultEffects`, `slowEffects`, plus `standard.*`) from `@nyanpasu/ui` so every `motion/react` component speaks the same physics language and respects `useReducedMotion()`.

---

## 3. Repository-Specific UI Contracts (Non-Negotiable)

These rules from `docs/development/typescript.md` stay strictly enforced alongside M3E:

1. **Dashboard widgets have no internal scrollbars:** Keep `min-h-0 overflow-hidden`; adapt visible rows with container queries (`@container`) and fold overflow into summary totals rather than `overflow-auto`.
2. **Menu items align to the start:** Leading icon + label, no `justify-between` or `ml-auto` pushing text to the far edge (only component-owned trailing indicators like submenu arrows or shortcut badges sit at the end).
3. **Modal lifecycle:** Compose with `Modal`, `ModalTrigger`, `ModalContent`, `ModalClose`, `ModalAnchor` from `@nyanpasu/ui/modal`. Keep roots and `ModalContent` mounted across open/close for smooth spring exit animations; never `{open && <Modal />}`; use `dismissible={!saving}` during async saves.
4. **Routed pages & layouts:** Use nested file routes with `AnimatedOutletPreset`; declare `staticData: { layout: 'fill' }` only for true internally-scrolling tool pages (`Connections`, `Logs`, `Global transforms`, `Inspect`). Keep page-local modules in the leaf page's `_modules/`.
5. **Capability gating:** Gate views and controls with `useCapabilities()`, `RequireCapability`, and `ReadOnlyScope` (`resource:access`), never role names.
6. **i18n discipline:** Keep Paraglide message keys grouped under their owning feature/component namespace across all locale files.

---

## 4. Synergy with `modern-web-guidance`

Before implementing or refactoring web CSS/DOM patterns in `@nyanpasu/ui` or `@nyanpasu/nyanpasu`, consult `.agents/skills/modern-web-guidance/SKILL.md` (`pnpx modern-web-guidance@latest search "<query>"`) especially for:

- Container queries (`@container`) and fluid scaling
- Entry/exit animations (`@starting-style`, `interpolate-size`, View Transitions)
- Native Popover / Dialog top-layer & Anchor Positioning
- `:user-invalid` form validation, scroll-driven effects, and custom scrollbar styling
- Accessibility (`prefers-reduced-motion`, `prefers-contrast`, `prefers-reduced-transparency`, focus management, contrast)
- Core Web Vitals / INP responsiveness in large lists (`content-visibility`, virtual lists)

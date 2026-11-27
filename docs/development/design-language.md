# Design language and color system

Clash Nyanpasu follows **Material 3 Expressive (2025 spec)** and the **Android 17** tonal surface language. Surface hierarchy is expressed through tonal contrast (`surface-container-*`) rather than drop shadows, hairline dividers, or opacity hacks.

## Package ownership

| Layer                              | Owning path                                                                               | Responsibility                                                                                                                                                                                                                                                                  |
| ---------------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dynamic color engine & design spec | `frontend/theme/src/index.ts`, `frontend/theme/src/spec.ts`, `frontend/theme/src/styles/` | Compute 49 M3E color roles from a seed color, harmonize syntax tokens, publish the machine-readable design spec (`SURFACE_LADDER`, `SHAPE_SCALE`, `M3E_MOTION_SPEC`, `CONTRAST_PAIRS`), and ship fallback CSS variables plus shared Tailwind v4 `@theme` tokens (`tokens.css`). |
| App Tailwind & motion entry        | `frontend/nyanpasu/src/assets/styles/tailwind.css`                                        | Import `@nyanpasu/theme/styles/tokens.css` and `@nyanpasu/ui/styles.css`, and add app-specific row animations and logo rules.                                                                                                                                                   |
| Theme provider & cache             | `frontend/nyanpasu/src/components/providers/theme-provider.tsx`                           | Resolve `light`/`dark`/`system` mode, inject generated CSS variables, and persist validated `v2` cache entries (`theme-palette-v2`, `theme-css-vars-v2`).                                                                                                                       |
| Reusable UI primitives             | `frontend/ui/src/`, `frontend/ui/src/styles.css`                                          | Enforce semantic surface, state-layer, glass, animation, and accessibility rules across all controls while preserving `data-slot` contracts.                                                                                                                                    |
| Living UI Gallery & spec explorer  | `frontend/gallery/src/`                                                                   | Standalone M3E/A17 workbench (`pnpm run gallery:dev`) consuming `@nyanpasu/theme` and `@nyanpasu/ui` as a pure external consumer.                                                                                                                                               |
| App chrome & pages                 | `frontend/nyanpasu/src/`                                                                  | Compose screens using the standardized tonal staircase below without introducing custom surface aliases or dark-mode role inversions.                                                                                                                                           |

## Dynamic color engine and spec selection

`@nyanpasu/theme` generates palettes via `@material/material-color-utilities` `DynamicScheme` rather than the legacy `themeFromSourceColor` helper:

- **Default scheme:** `SchemeTonalSpot` with `specVersion: '2025'` and `contrastLevel: 0` (default seed `#3e64a7`). `SchemeTonalSpot` stays faithful to the user's seed color and keeps neutral surfaces calm for dense productivity screens (tables, logs, settings, editors).
- **Extensible options:** `createTheme(color, { variant, contrastLevel, specVersion })` accepts all MCU variants (`tonalSpot`, `expressive`, `fidelity`, `content`, `neutral`, `monochrome`, `vibrant`, `rainbow`, `fruitSalad`), contrast levels, and `'2025' | '2021'` specs so future appearance settings can expose them without changing the theme contract.
- **Emitted roles:** All 49 M3E color roles are generated for both `light` and `dark`, including `surface-dim`, `surface-bright`, the 5-level `surface-container-{lowest,low,default,high,highest}` scale, `surface-tint`, and all `primary`/`secondary`/`tertiary` `*-fixed*` roles.
- **Fallback and caching:** `frontend/theme/src/styles/theme.css` ships the exact `DEFAULT_COLOR` 2025 fallback values for first paint and tests. `theme-provider.tsx` validates cached palettes with `isValidThemePalette()` so stale or incomplete caches fall back safely.

## Standardized A17 tonal surface ladder

Every adjacent region in the application steps monotonically across M3E surface tones so chrome, navigation, canvas, and cards separate cleanly in both modes without border clutter:

| UI region                                                  | Light mode (tone)                | Dark mode (tone)                 | Tailwind classes                                        |
| ---------------------------------------------------------- | -------------------------------- | -------------------------------- | ------------------------------------------------------- |
| Window titlebar (`WindowHeader`)                           | `primary-container` (80)         | `primary-container` (35)         | `bg-primary-container text-on-primary-container`        |
| Top navigation bar & editor toolbar (`AppNavbar`, toolbar) | `secondary-container` (90)       | `secondary-container` (25)       | `bg-secondary-container text-on-secondary-container`    |
| Collapsible navigation rail (`NavigationSidebar`)          | `surface-container` (94)         | `surface-container-low` (10)     | `bg-surface-container dark:bg-surface-container-low`    |
| Page canvas & split-pane sidebars                          | `surface-container-low` (96)     | `surface` (6)                    | `bg-surface-container-low dark:bg-surface`              |
| Cards, grouped rows, tiles & inactive switches             | `surface-container-lowest` (100) | `surface-container` (14)         | `bg-surface-container-lowest dark:bg-surface-container` |
| Nested sub-cards / inner wells inside a card               | `surface-container` (94)         | `surface-container-high` (17)    | `bg-surface-container dark:bg-surface-container-high`   |
| Tool-page side panes (Inspector / Global transforms)       | `surface-container-low` (96)     | `surface-container-low` (10)     | `bg-surface-container-low`                              |
| Elevated popovers, menus, dialogs & raised cards           | `surface-container-high` (92)    | `surface-container-high` (17)    | `bg-surface-container-high`                             |
| Filled inputs, search fields, tracks & neutral badges      | `surface-container-highest` (90) | `surface-container-highest` (22) | `bg-surface-container-highest`                          |
| Code, YAML & diff editor ground                            | `surface-container-lowest` (100) | `surface-container-lowest` (0)   | `bg-surface-container-lowest`                           |
| Modal & drawer backdrop scrim                              | `scrim` at 32%                   | `scrim` at 32%                   | `bg-scrim/32`                                           |
| Tooltips & transient high-contrast feedback                | `inverse-surface`                | `inverse-surface`                | `bg-inverse-surface text-inverse-on-surface`            |

### Why light and dark polarity differ on cards

In light mode, pure-white `surface-container-lowest` cards float crisply over a bright `surface-container-low` canvas with zero drop shadows. In dark mode, `surface-container` cards lift clearly above a near-black `surface` canvas. Both directions mirror Android 17 Settings while keeping neighboring chrome tiers (`WindowHeader` -> `AppNavbar` -> `NavigationSidebar` -> canvas) distinct without hairline borders.

## Color pairing, states, and borders

- **Pair `X` with `on-X`:** Always pair `primary` with `on-primary`, `primary-container` with `on-primary-container`, `secondary-container` with `on-secondary-container`, `tertiary-container` with `on-tertiary-container`, `error-container` with `on-error-container`, and `surface-container-*` with `on-surface` / `on-surface-variant`.
- **Active navigation and selection pills:** On the brand-tinted `AppNavbar` (`bg-secondary-container`), use `bg-primary-container text-on-primary-container` for the active pill. Inside neutral workspace navigation rails and lists (`NavigationSidebar`, `NavigationList`), use `bg-secondary-container text-on-secondary-container` for active items. Reserve `bg-primary text-on-primary` and `bg-primary-container text-on-primary-container` for primary CTAs, FABs, and active toggles.
- **State layers:** Use `hover:bg-on-surface/8` for neutral interactive hover states instead of swapping to `bg-surface-variant` or `bg-primary-container`. Use `bg-on-surface/12` and `text-on-surface/38` for disabled controls.
- **Outline roles:** Use `border-outline-variant` (or `/40`–`/50`) for decorative dividers and subtle card/menu frames. Use `border-outline` for meaningful interactive boundaries such as outlined buttons, text fields, checkboxes, and unchecked switches.
- **Button layout:** `Button` defaults to `inline-flex items-center justify-center gap-2 whitespace-nowrap` with non-shrinking icons (`[&_svg]:shrink-0`) so icon + label buttons stay on a single line without wrapping. Opt into `whitespace-normal` only for explicit multi-line controls.

## Android 17 frosted translucency

Floating chrome overlays (such as the `DataPage` bottom dock and floating pill controls) use `glassSurfaceVariants` from `@nyanpasu/ui/glass-surface`:

- Dynamic-tinted translucency (`supports-[backdrop-filter]:bg-surface-container-high/76 supports-[backdrop-filter]:backdrop-blur-xl`) with a subtle `border-outline-variant/40` edge.
- Mandatory solid `bg-surface-container-high` fallbacks for browsers without `backdrop-filter`, `prefers-reduced-transparency: reduce`, and `prefers-contrast: more`, plus `motion-reduce:backdrop-blur-none`.
- Never apply frosted blur to opaque reading panes, settings lists, or full-screen modal scrims.

## Permitted non-dynamic color exceptions

The following colors intentionally stay outside the dynamic surface ladder:

1. **Fixed per-item icon hues:** `--color-md-{red,orange,yellow,green,cyan,blue,purple,pink}-container` (and `on-*-container`) in `frontend/theme/src/styles/theme.css`, used so navigation icon badges stay visually distinct regardless of the chosen seed color.
2. **Harmonized syntax highlighting:** `--syntax-*` variables generated by `getSyntaxColors()` in `@nyanpasu/theme` for Monaco and Shiki.
3. **Status and data visualization:** `--color-tc`, `--color-ok`, and `--color-warn` in `tailwind.css`, topology/traffic charts, and provider/brand icons.
4. **Color-picker swatches:** Literal swatch colors and contrast rings inside `HctColorPicker`.

## Must not (anti-patterns)

- **No dark-mode role inversions:** Never use `bg-primary-container dark:bg-on-primary`, `bg-primary dark:bg-primary-container`, or `bg-inverse-on-surface dark:bg-surface` as surfaces.
- **No raw neutral literals:** Do not use `bg-zinc-*`, `bg-gray-*`, `bg-slate-*`, `text-zinc-*`, `bg-white`, `bg-black`, or hardcoded `#hex` backgrounds in UI components or app chrome.
- **No ad-hoc surface aliases:** Do not reintroduce `--color-mixed-background`, `--color-pane`, `--color-code`, or `--background-color` / `--scroller-color` root variables; use canonical M3E surface roles directly.
- **No `bg-surface-variant` surface hacks:** Keep `text-on-surface-variant` for muted copy, but use `surface-container-*` for container fills and `outline-variant` for borders.

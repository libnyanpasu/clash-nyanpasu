# M3 Expressive Design Tokens

Exact published Material 3 Expressive values (from `m3.material.io` and `androidx.compose.material3.tokens`). Copy exact values; do not round or guess.

---

## 1. Color System & Tonal Hierarchy

Powered by Material Color Utilities (HCT color space: Hue `0-360`, Chroma `0-120`, Tone `0-100`). Generate schemes with `DynamicScheme` / `SchemeTonalSpot` (or `SchemeExpressive` for vibrant accents) and `MaterialDynamicColors` — **not** the legacy `themeFromSourceColor()` helper, which omits the 5 `surfaceContainer*` and `fixed` roles.

### Surface & Tonal Container Roles (Mandatory)

Surface hierarchy in M3E is **tone-based**, not shadow- or opacity-overlay-based.

| Token                     | CSS Variable Convention                | Light Tone | Dark Tone | Intended Role                                                |
| ------------------------- | -------------------------------------- | ---------- | --------- | ------------------------------------------------------------ |
| `surfaceDim`              | `--color-md-surface-dim`               | 87         | 6         | Recessed backdrop / dimmed canvas behind elevated sheets     |
| `surface`                 | `--color-md-surface`                   | 98         | 6         | Main page body canvas                                        |
| `surfaceBright`           | `--color-md-surface-bright`            | 98         | 24        | Highlighted neutral surface (dark-mode lift)                 |
| `surfaceContainerLowest`  | `--color-md-surface-container-lowest`  | 100        | 4         | Deepest recessed container (code/editor ground, nested well) |
| `surfaceContainerLow`     | `--color-md-surface-container-low`     | 96         | 10        | Subtle grouped cards, elevated buttons, settings groups      |
| `surfaceContainer`        | `--color-md-surface-container`         | 94         | 12        | Default container tier: navigation rail/bar, app bars, panes |
| `surfaceContainerHigh`    | `--color-md-surface-container-high`    | 92         | 17        | Prominent cards, dialogs, menus, popovers, sheets            |
| `surfaceContainerHighest` | `--color-md-surface-container-highest` | 90         | 22        | Active neutral fills, filled text fields, off-switch tracks  |

### Accent, Inverse, Outline, and Fixed Roles

| Group              | Roles                                                                                                                                                                               | Usage                                                                                                                  |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Primary**        | `primary`, `on-primary`, `primary-container`, `on-primary-container`                                                                                                                | Highest-emphasis actions, active selection pills, primary FABs, active switches.                                       |
| **Secondary**      | `secondary`, `on-secondary`, `secondary-container`, `on-secondary-container`                                                                                                        | Medium-emphasis actions, tonal buttons, active nav indicators, filter chips.                                           |
| **Tertiary**       | `tertiary`, `on-tertiary`, `tertiary-container`, `on-tertiary-container`                                                                                                            | Contrasting accents for small focal points, status callouts, badges, secondary highlights.                             |
| **Error**          | `error`, `on-error`, `error-container`, `on-error-container`                                                                                                                        | Static semantic error palette (adapts to light/dark, never shifts hue with seed).                                      |
| **Fixed**          | `primary-fixed`, `primary-fixed-dim`, `on-primary-fixed`, `on-primary-fixed-variant` (and secondary/tertiary equivalents)                                                           | Accent fills that keep identical tone across light and dark modes (e.g., media overlays, badges over imagery).         |
| **Inverse**        | `inverse-surface`, `inverse-on-surface`, `inverse-primary`                                                                                                                          | High-contrast transient feedback (snackbars, rich tooltips).                                                           |
| **Outline**        | `outline` (meaningful interactive boundaries, e.g., text fields, outlined buttons); `outline-variant` (decorative dividers, subtle card borders where content already meets 4.5:1). | Never use `outline` for decorative dividers; never rely solely on `outline-variant` to define a touch target boundary. |
| **Scrim / Shadow** | `scrim`, `shadow`                                                                                                                                                                   | Modal backdrops (`scrim` at ~32% opacity) and subtle elevation shadows.                                                |

### Pairing & Contrast Rules

- Always pair `X` with `on-X`. Never put `primary` text on `primary-container` (contrast breaks across contrast levels and dynamic seeds).
- Minimum contrast: **4.5:1** for body/small text, **3:1** for large text and UI components.
- Support three contrast levels (`standard`, `medium` >= 3:1, `high` >= 7:1 on content).
- Keep layout-region surface roles consistent across breakpoints (body = `surface`, navigation = `surface-container` on both compact and expanded screens).

### State Layer Opacities

Apply interactive state overlays using the element's `on-*` color via `color-mix(in oklab, ...)`:

| State                        | Opacity                                     |
| ---------------------------- | ------------------------------------------- |
| Hover                        | `8%`                                        |
| Focus                        | `10%` (plus visible `2px`–`3px` focus ring) |
| Pressed                      | `10%`                                       |
| Dragged                      | `16%`                                       |
| Disabled container           | `10%`–`12%` of `on-surface`                 |
| Disabled content (text/icon) | `38%` of `on-surface`                       |

---

## 2. Shape Scale (10 Steps) & Optical Roundness

Bold entries were added by the M3 Expressive update (May 2025).

| Token                     | Value      | Web / Tailwind Mapping       | Typical Usage                                                                     |
| ------------------------- | ---------- | ---------------------------- | --------------------------------------------------------------------------------- |
| `none`                    | `0px`      | `0px` (`rounded-none`)       | Full-bleed bars, sharp tables, code gutters                                       |
| `extraSmall`              | `4px`      | `0.25rem` (`rounded-xs`)     | Tight chips, inner corners of connected XS groups, grouped list inner corners     |
| `small`                   | `8px`      | `0.5rem` (`rounded-sm`)      | Text fields, menus, pressed XS/S buttons, connected S/M inner corners             |
| `medium`                  | `12px`     | `0.75rem` (`rounded-md`)     | Standard cards, square XS/S buttons, pressed M buttons, small FAB                 |
| `large`                   | `16px`     | `1rem` (`rounded-lg`)        | FAB, square M buttons, pressed L/XL buttons, grouped settings sections, drawers   |
| **`largeIncreased`**      | **`20px`** | **`1.25rem` (`rounded-xl`)** | Intermediate expressive cards, split panels, connected XL inner corners           |
| `extraLarge`              | `28px`     | `1.75rem` (`rounded-2xl`)    | Dialogs, bottom sheets, large FAB, square L/XL buttons, hero cards                |
| **`extraLargeIncreased`** | **`32px`** | **`2rem` (`rounded-3xl`)**   | Expressive hero containers, floating modal sheets                                 |
| **`extraExtraLarge`**     | **`48px`** | **`3rem`**                   | Maximum expressive hero framing                                                   |
| `full`                    | `9999px`   | `9999px` (`rounded-full`)    | Round buttons, pills, floating toolbars, active nav indicators, avatars, switches |

### Mandatory Optical Roundness Formula

When nesting a rounded child inside a padded rounded container, never copy the parent radius:

```text
innerRadius = max(4px, outerRadius - padding)
Example: 28px outer card - 12px padding = 16px inner tile
```

### Grouped List & Asymmetric Inner Corners (Android 17 / M3E Signature)

For stacked settings rows or grouped items inside a section:

- First item: `rounded-t-[16px] rounded-b-[4px]` (or `20px` outer / `4px` inner for spacious sections)
- Middle items: `rounded-[4px]`
- Last item: `rounded-t-[4px] rounded-b-[16px]`
- Single item: `rounded-[16px]` (or `20px`)
- Gap between items: `2px` (`gap-0.5`), revealing the underlying `surface` canvas without heavy horizontal dividers.

### Abstract Material Shapes (35 Shapes)

Available conceptual library: `Circle`, `Square`, `Slanted`, `Arch`, `Fan`, `Arrow`, `SemiCircle`, `Oval`, `Pill`, `Triangle`, `Diamond`, `ClamShell`, `Pentagon`, `Gem`, `Sunny`, `VerySunny`, `Cookie4/6/7/9/12Sided`, `Clover4/8Leaf`, `Ghostish`, `Burst`, `SoftBurst`, `Boom`, `SoftBoom`, `Flower`, `Puffy`, `PuffyDiamond`, `PixelCircle`, `PixelTriangle`, `Bun`, `Heart`.

- Use **only** for decorative accents, loading indicators, avatars, status emblems, and 1–2 hero widgets.
- **Never** apply abstract polygon clips to text-heavy containers or form controls.

---

## 3. Typography Scale (30 Styles: 15 Baseline + 15 Emphasized)

Scale ratio: Major Second (`1.125`), base `14px`. Emphasized styles keep the exact same font size and line height as baseline; only **weight** and **tracking** change.

### Baseline vs. Emphasized Table

| Role              | Size                 | Line Height | Baseline Weight / Tracking | Emphasized Weight / Tracking | Typical Assignment                                |
| ----------------- | -------------------- | ----------- | -------------------------- | ---------------------------- | ------------------------------------------------- |
| `display-large`   | `57px` (`3.5625rem`) | `64px`      | `400` / `-0.2px`           | `500` / `0px`                | Hero metrics, splash banners                      |
| `display-medium`  | `45px` (`2.8125rem`) | `52px`      | `400` / `0px`              | `500` / `0px`                | Large dashboard hero figures                      |
| `display-small`   | `36px` (`2.25rem`)   | `44px`      | `400` / `0px`              | `500` / `0px`                | Prominent numeric callouts                        |
| `headline-large`  | `32px` (`2rem`)      | `40px`      | `400` / `0px`              | `500` / `0px`                | Expanded page headers                             |
| `headline-medium` | `28px` (`1.75rem`)   | `36px`      | `400` / `0px`              | `500` / `0px`                | Page titles, flexible app bar expanded            |
| `headline-small`  | `24px` (`1.5rem`)    | `32px`      | `400` / `0px`              | `500` / `0px`                | Dialog headings, section heroes                   |
| `title-large`     | `22px` (`1.375rem`)  | `28px`      | `400` / `0px`              | `500` / `0px`                | Top app bar title (collapsed), sheet titles       |
| `title-medium`    | `16px` (`1rem`)      | `24px`      | `500` / `0.2px`            | **`700`** / `0.15px`         | Card titles, settings group headers               |
| `title-small`     | `14px` (`0.875rem`)  | `20px`      | `500` / `0.1px`            | **`700`** / `0.1px`          | Subheaders, dense card headers, active nav items  |
| `body-large`      | `16px` (`1rem`)      | `24px`      | `400` / `0.5px`            | `500` / `0.15px`             | Primary list item titles, readable body copy      |
| `body-medium`     | `14px` (`0.875rem`)  | `20px`      | `400` / `0.2px`            | `500` / `0.25px`             | Standard UI copy, descriptions, table cells       |
| `body-small`      | `12px` (`0.75rem`)   | `16px`      | `400` / `0.4px`            | `500` / `0.4px`              | Supporting copy, timestamps, secondary metadata   |
| `label-large`     | `14px` (`0.875rem`)  | `20px`      | `500` / `0.1px`            | **`700`** / `0.1px`          | Buttons, tabs, navigation labels, prominent chips |
| `label-medium`    | `12px` (`0.75rem`)   | `16px`      | `500` / `0.5px`            | **`700`** / `0.5px`          | Nav bar destinations, compact buttons, badges     |
| `label-small`     | `11px` (`0.6875rem`) | `16px`      | `500` / `0.5px`            | **`700`** / `0.5px`          | Micro badges, compact table headers, hints        |

- Use `font-variant-numeric: tabular-nums` on all live counters, speeds, latencies, and timestamps (project design decision for network-monitoring clarity).
- Use variable font optical sizing (`font-optical-sizing: auto`) when the active font supports it (`Inter Variable`, `Roboto Flex`).

---

## 4. Motion Physics System (Springs)

M3 Expressive **replaces** legacy duration/easing curves with spring physics (`stiffness`, `dampingRatio`, `mass: 1`).

### Exact Compose Spring Parameters

| Token             | Expressive Scheme (Default)            | Standard Scheme (Utilitarian)          | Animates                                                         |
| ----------------- | -------------------------------------- | -------------------------------------- | ---------------------------------------------------------------- |
| `fast.spatial`    | `dampingRatio: 0.6`, `stiffness: 800`  | `dampingRatio: 0.9`, `stiffness: 1400` | Small controls: button morph, switch thumb, chip toggle          |
| `default.spatial` | `dampingRatio: 0.8`, `stiffness: 380`  | `dampingRatio: 0.9`, `stiffness: 700`  | Partial-screen movement: sheets, dialogs, nav rail expand, cards |
| `slow.spatial`    | `dampingRatio: 0.8`, `stiffness: 200`  | `dampingRatio: 0.9`, `stiffness: 300`  | Full-screen route/container transitions                          |
| `fast.effects`    | `dampingRatio: 1.0`, `stiffness: 3800` | `dampingRatio: 1.0`, `stiffness: 3800` | Small color/opacity state changes                                |
| `default.effects` | `dampingRatio: 1.0`, `stiffness: 1600` | `dampingRatio: 1.0`, `stiffness: 1600` | Panel/dialog opacity, tint, blur fade                            |
| `slow.effects`    | `dampingRatio: 1.0`, `stiffness: 800`  | `dampingRatio: 1.0`, `stiffness: 800`  | Full-screen fade / refresh                                       |

### Converting `dampingRatio` to `motion/react` (`damping`)

`motion/react` spring physics uses `stiffness`, `damping`, and `mass` (default `1`), where `damping = 2 * dampingRatio * sqrt(stiffness * mass)`:

| Token             | `motion/react` Expressive                                      | `motion/react` Standard                                       |
| ----------------- | -------------------------------------------------------------- | ------------------------------------------------------------- |
| `fast.spatial`    | `{ type: 'spring', stiffness: 800, damping: 33.9, mass: 1 }`   | `{ type: 'spring', stiffness: 1400, damping: 67.3, mass: 1 }` |
| `default.spatial` | `{ type: 'spring', stiffness: 380, damping: 31.2, mass: 1 }`   | `{ type: 'spring', stiffness: 700, damping: 47.6, mass: 1 }`  |
| `slow.spatial`    | `{ type: 'spring', stiffness: 200, damping: 22.6, mass: 1 }`   | `{ type: 'spring', stiffness: 300, damping: 31.2, mass: 1 }`  |
| `fast.effects`    | `{ type: 'spring', stiffness: 3800, damping: 123.3, mass: 1 }` | Same                                                          |
| `default.effects` | `{ type: 'spring', stiffness: 1600, damping: 80.0, mass: 1 }`  | Same                                                          |
| `slow.effects`    | `{ type: 'spring', stiffness: 800, damping: 56.6, mass: 1 }`   | Same                                                          |

### CSS Cubic-Bezier Fallbacks (Non-Interruptible CSS Transitions)

Use when CSS transitions (e.g., `border-radius`, `background-color`, `transform`) cannot use a JS spring engine:

| Spring                     | `cubic-bezier(...)`                    | Duration |
| -------------------------- | -------------------------------------- | -------- |
| Expressive fast spatial    | `cubic-bezier(0.42, 1.67, 0.21, 0.90)` | `350ms`  |
| Expressive default spatial | `cubic-bezier(0.38, 1.21, 0.22, 1.00)` | `500ms`  |
| Expressive slow spatial    | `cubic-bezier(0.39, 1.29, 0.35, 0.98)` | `650ms`  |
| Expressive fast effects    | `cubic-bezier(0.31, 0.94, 0.34, 1.00)` | `150ms`  |
| Expressive default effects | `cubic-bezier(0.34, 0.80, 0.34, 1.00)` | `200ms`  |
| Expressive slow effects    | `cubic-bezier(0.34, 0.88, 0.34, 1.00)` | `300ms`  |
| Standard fast spatial      | `cubic-bezier(0.27, 1.06, 0.18, 1.00)` | `350ms`  |
| Standard default spatial   | `cubic-bezier(0.27, 1.06, 0.18, 1.00)` | `500ms`  |
| Standard slow spatial      | `cubic-bezier(0.27, 1.06, 0.18, 1.00)` | `750ms`  |

- **Reduced motion (`prefers-reduced-motion: reduce`):** disable spatial bounce/translation (`duration: 0` or instant state swaps) while keeping minimal opacity/color feedback so state changes remain legible. (Product design decision; M3 does not prescribe a specific reduced-motion swap table.)

---

## 5. Elevation Levels

Six elevation tiers (`0 / 1 / 3 / 6 / 8 / 12 dp`). Prefer tonal `surface-container-*` shifts for resting surfaces; reserve shadow elevation (`Level 1`–`Level 3`) for floating chrome (FABs, floating toolbars, menus, popovers, dragged cards).

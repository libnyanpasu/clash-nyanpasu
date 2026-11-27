# M3 Expressive Component Catalog & Selection Rules

Exact component semantics, sizes, shapes, and interaction rules from M3 / M3 Expressive specifications.

---

## 1. Buttons (10 Types, 5 Sizes, Shape Morphing)

### Emphasis Hierarchy (Choose by Role, Not Decoration)

| Emphasis    | Component                                                                                                                                         | Purpose                                                                                                                      |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **Highest** | FAB / Extended FAB / FAB Menu                                                                                                                     | Single primary constructive action on the screen (e.g., Create profile, Add rule). Never destructive or minor.               |
| **High**    | Filled Button / Split Button / Standard Button Group                                                                                              | Primary CTA (`primary` + `on-primary`), primary action with variants (Split), or hero multi-action cluster (Standard Group). |
| **Medium**  | Tonal Button (`secondary-container`), Elevated Button (`surface-container-low` + Level 1 shadow), Outlined Button (`outline-variant` / `outline`) | Supporting actions, secondary confirmations, escape/cancel flows (`outlined`).                                               |
| **Low**     | Connected Button Group, Text Button, Icon Button                                                                                                  | View/mode switching (Connected Group), supplementary links (`text`), compact icon triggers (`icon`).                         |

In two-button rows (LTR), place the **lower-emphasis** button on the left and the **higher-emphasis** button on the right (e.g., `Text / Outlined` Cancel + `Filled` Save).

### 5 Size Tiers & Exact Geometry (Compose M3E Tokens)

| Size            | Height  | Round Resting | Square Resting        | Pressed Shape     | Icon Size | Horizontal Padding      | Icon-Label Gap |
| --------------- | ------- | ------------- | --------------------- | ----------------- | --------- | ----------------------- | -------------- |
| **XS**          | `32px`  | `full`        | `12px` (`medium`)     | `8px` (`small`)   | `20px`    | `16px` (`12px` compact) | `8px`          |
| **S** (Default) | `40px`  | `full`        | `12px` (`medium`)     | `8px` (`small`)   | `20px`    | `16px`–`24px`           | `8px`          |
| **M**           | `56px`  | `full`        | `16px` (`large`)      | `12px` (`medium`) | `24px`    | `24px`                  | `8px`          |
| **L**           | `96px`  | `full`        | `28px` (`extraLarge`) | `16px` (`large`)  | `32px`    | `48px`                  | `12px`         |
| **XL**          | `136px` | `full`        | `28px` (`extraLarge`) | `16px` (`large`)  | `40px`    | `64px`                  | `16px`         |

### Button Shape Morphing (Mandatory M3E Signature)

- **Pressed state:** Both `round` and `square` buttons morph smoothly (`fast.spatial` spring) toward their **pressed corner radius** (`8px` for XS/S, `12px` for M, `16px` for L/XL).
- **Toggle buttons:** Swap resting shape on selection — `round` when unselected -> `square` when selected (or vice versa), paired with color-role shift (e.g., `surface-container` / `tonal` -> `primary` or `secondary-container`).

---

## 2. Button Groups & Split Button

### Connected Button Group (Replaces Legacy Segmented Button)

Use for single-select or multi-select view/mode/sort switching (e.g., Proxy Mode `Rule / Global / Direct`, Chart time range, View toggle).

- **Gap between items:** Always `2px` (`gap-0.5`) at every size.
- **Outer corners:** `full` (round variant) or `4 / 8 / 8 / 16 / 20px` (square variant across XS–XL).
- **Inner corners:** `4px` (XS), `8px` (S, M), `16px` (L), `20px` (XL).
- **Selected item:** Morphs to `full` (round) corners (or swaps to square in a round group) and shifts to emphasized fill (`primary` / `secondary-container`) + emphasized label weight. Does **not** push adjacent buttons' widths.
- **Touch target:** XS and S connected groups maintain a `48px` minimum hit area / `48px` minimum button width.
- Never wrap a button group onto a second line; collapse overflow into a trailing menu on narrow viewports.
- Never mix color styles inside a connected group.

### Standard Button Group (Hero Multi-Action)

Use for closely related primary actions (e.g., playback/control clusters, core actions):

- Inner gap scales by size to preserve 48px touch targets: XS `18px`, S `12px`, M/L/XL `8px`.
- When one button is pressed or toggled, it morphs shape and briefly expands its width while **adjacent buttons yield/compress slightly** using `fast.spatial` spring motion.

### Split Button

Pairs a primary action (leading segment) with a dropdown/options menu trigger (trailing segment) separated by a `2px` gap:

- Outer ends are `full` rounded; inner corners facing the `2px` gap are smaller (`4px`–`8px` depending on size).
- Opening the menu rotates the trailing chevron `180deg` and morphs the trailing button's corners toward `full` circle.

---

## 3. FAB, Extended FAB & FAB Menu

- **Sizes:** Standard `56x56px` (`16px` radius, `24px` icon), **Medium `80x80px`** (`20px` radius, `28px` icon — M3E recommended default for mobile/medium), Large `96x96px` (`28px` radius, `32px` icon). **Small FAB (`40px`) is deprecated.**
- **Colors:** `primary-container`, `secondary-container`, `tertiary-container`, or new M3E high-emphasis `primary`, `secondary`, `tertiary`.
- **Placement:** Bottom-trailing corner on compact/medium windows (above bottom nav bar); top-leading inside the Navigation Rail on expanded/desktop windows.
- **FAB Menu:** Clicking the FAB morphs it into a circular close toggle while revealing a vertical stack of pill-shaped tonal menu items with `fast.spatial` spring entrance.

---

## 4. Navigation (Bar, Rail, Expanded Rail, Tabs)

**Legacy Navigation Drawer is no longer recommended in M3E.** Replace it with an **Expanded Navigation Rail** that smoothly toggles between collapsed rail and expanded rail (or modal expanded rail on compact screens).

| Window Size Class                      | Primary Navigation Pattern                                                                                                                                                                      |
| -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Compact (`< 600px`)**                | **Bottom Navigation Bar** (3–5 equal top-level destinations; pill active indicator; filled active icon + outlined inactive icon) OR **Modal Expanded Navigation Rail** (when > 5 destinations). |
| **Medium (`600–839px`)**               | **Navigation Rail** (collapsed `80px`–`96px` vertical rail, or expandable) OR horizontal-item Navigation Bar.                                                                                   |
| **Expanded / Large / XL (`>= 840px`)** | **Navigation Rail** (collapsible <-> expanded rail, `220px`–`260px` expanded).                                                                                                                  |

- **Never** display two primary navigation components simultaneously (e.g., no bottom bar + side rail together).
- **Tabs** are strictly for switching related views _within_ one page, never for unrelated top-level routes. Use sliding pill or primary-indicator tabs with `fast.spatial` spring tracking.
- **Badges:** Small dot (`6px`, `error` fill) for unread/status ping; Large pill (`16px` height, max 4 chars e.g. `99+`, `error` + `on-error`) for counts. Hide unread badges once visited.

---

## 5. App Bars & Floating Toolbars

### App Bars (Top)

- **Small (`64px`):** Single-row title (`title-large`) + leading nav/back + trailing actions.
- **Medium Flexible / Large Flexible:** Spacious header (`headline-medium` / `headline-large` emphasized) that compresses smoothly into a small app bar on scroll.
- **Scroll separation:** Starts matching `surface`; transitions to `surface-container` (or frosted translucent bar in Android 17 style) on scroll. **No drop shadow.**
- **Overflow:** Trailing actions collapse into an overflow menu on compact widths.

### Floating Toolbar (M3E Signature)

Floating pill container (`full` radius, `surface-container-high` or Android 17 frosted glass, Level 3 shadow) hovering near the bottom edge (or section edge) for contextual page tools (e.g., search + filter + sort + bulk actions in Proxies / Connections / Logs / Editor):

- Supports **Standard** (neutral `surface-container` fill) and **Vibrant** (`primary-container` fill) color styles.
- Horizontal or vertical orientation; can pair with an adjacent FAB.
- Keep concise (3–5 primary controls + overflow).

---

## 6. Feedback: Loading Indicator vs. Progress Indicator

Strict wait-time decision rule:

| Expected Wait        | Component                  | Behavior                                                                                                                                                |
| -------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `< 200ms`            | None                       | Instant transition; avoid spinner flicker.                                                                                                              |
| `200ms – 5s`         | **M3E Loading Indicator**  | Rotating morphing polygon shape (`MaterialShape` sequence cycling through cookie/clover/pill/soft-burst) inside an optional `primary-container` circle. |
| `> 5s` or measurable | **M3E Progress Indicator** | Wavy or flat linear/circular track with stop indicator dot; transitions indeterminate -> determinate when progress arrives.                             |

**Never** transition a Loading Indicator into a Progress Indicator mid-operation.

---

## 7. Containment, Inputs & Overlays

- **Cards:** Three variants: `Elevated` (`surface-container-low` + Level 1 shadow), `Filled` (`surface-container-highest` or `surface-container-low` on `surface`), `Outlined` (`surface` + `1px outline-variant`). Prefer **Filled** or **Outlined** tonal cards over heavy shadows. Never wrap every row in a card.
- **Lists & Settings Groups:** Group related settings rows inside a single tonal section using Android 17 / M3E asymmetric radii (`16px`–`20px` outer ends, `4px` inner corners, `2px` row gap) on `surface-container-low` / `surface-container`. Minimum row height `56px` (one-line) / `72px` (two-line) on touch layouts.
- **Switch:** `52x32px` pill track (`surface-container-highest` + `outline` border when off; `primary` fill when on); thumb expands from `16px` (off) -> `24px` (on, with check icon) -> `28px` (pressed) using `fast.spatial` spring.
- **Checkbox vs. Radio vs. Switch:** Switch = immediate on/off setting; Checkbox = multi-select or parent/indeterminate selection; Radio = mutually exclusive choice (`<= 5` options, vertical stack).
- **Sliders:** Thick expressive track (`16px`–`24px` M3E active track with rounded outer corners and `4px` gap beside a tall `4px` vertical handle pill) + floating value pill tooltip while dragging.
- **Dialogs & Sheets:** Dialogs use `extraLarge` (`28px`) corners on `surface-container-high` (or frosted translucent overlay), `headline-small` title, and trailing-aligned actions. On compact/mobile viewports, prefer **Bottom Sheets** (`28px` top corners, `32x4px` drag handle) over cramped centered dialogs for pickers, filters, and quick editors.
- **Menus & Popovers:** `12px`–`16px` corners (`medium`/`large`), `surface-container` or frosted glass, start-aligned icon + label items (never push text to the far edge with `justify-between` unless an owned submenu chevron/shortcut).
- **Snackbars:** `inverse-surface` + `inverse-on-surface` + `inverse-primary` action button, positioned above bottom navigation / floating toolbars.

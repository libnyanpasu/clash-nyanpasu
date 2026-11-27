# Android 17 Design Language & Adaptive Mobile Readiness

Guidance for combining Android 17's frosted-glass spatial layering with Material 3 Expressive adaptive layouts across desktop, tablet, and mobile form factors.

---

## 1. Android 17 Frosted Translucency & Depth

Android 17 evolves Material 3 Expressive from flat opaque chrome to **dynamic-color-tinted frosted translucency** so users retain spatial context of underlying content while focusing on foreground chrome.

### Where Translucency Belongs (and Where It Does Not)

| Surface Class                                                                                                                                                                 | Treatment                                                                                                                                                                                                                                                                | Rationale                                                                       |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| **Floating Chrome & Overlays** (floating toolbars, sticky scrolled app bars, menus, popovers, dialogs, bottom sheets, quick mode switchers, tooltips, floating pill controls) | **Dynamic-tinted frosted glass:** `color-mix(in oklab, var(--color-surface-container-high) 76%, transparent)` + `backdrop-filter: blur(20px) saturate(1.4)` + subtle `1px` specular/outline edge (`color-mix(in oklab, var(--color-outline-variant) 45%, transparent)`). | Preserves live visual context underneath while keeping crisp foreground focus.  |
| **Main Reading & Working Surfaces** (page body, proxy grids, settings lists, connection tables, log viewers, code editors)                                                    | **Solid tonal M3E surfaces:** `surface`, `surface-container-low`, `surface-container`.                                                                                                                                                                                   | Maximum contrast, legibility, and rendering performance during rapid scrolling. |
| **Persistent Navigation Chrome** (navigation rail / bottom bar)                                                                                                               | Solid `surface-container` at rest, OR subtle frosted tint when content scrolls edge-to-edge underneath a floating bottom navigation bar.                                                                                                                                 | Never blur behind static empty margins where nothing scrolls underneath.        |

### Mandatory Translucency Guardrails

1. **Dynamic tint, never gray fog:** Always mix from semantic M3 tokens (`surface-container`, `surface-container-high`, `primary-container`), never `rgba(255,255,255,0.5)` or `rgba(0,0,0,0.5)`.
2. **Contrast guarantee:** Foreground text/icons over frosted chrome MUST still satisfy `>= 4.5:1` contrast regardless of what scrolls behind it. Keep tint opacity high enough (`72%–84%` light, `74%–86%` dark) and use generous blur (`16px–24px`).
3. **Accessibility fallback (Mandatory):** Provide a solid surface fallback when `@media (prefers-reduced-transparency: reduce)` or `@media (prefers-contrast: more)` matches, or when `backdrop-filter` is unsupported (`@supports not (backdrop-filter: blur(1px))`).
4. **Performance discipline:** Never stack nested `backdrop-filter` layers inside virtualized list rows or dashboard widget cards. Limit active blurred surfaces to floating chrome and overlays.

---

## 2. Adaptive Window Size Classes

Design for the **available viewport width**, not a guessed device type or OS string.

| Class           | Width Range       | Layout Structure                                             | Navigation                                               | Margins / Pane Gap              |
| --------------- | ----------------- | ------------------------------------------------------------ | -------------------------------------------------------- | ------------------------------- |
| **Compact**     | `< 600px`         | Single-pane; modals become Bottom Sheets                     | Bottom Navigation Bar (3–5 items) or Modal Expanded Rail | `16px` margin                   |
| **Medium**      | `600px – 839px`   | Single-pane or supporting-pane; collapsible rail             | Collapsed Navigation Rail (`80px`)                       | `24px` margin                   |
| **Expanded**    | `840px – 1199px`  | Two-pane (List-Detail or Supporting Pane)                    | Navigation Rail (collapsible <-> expanded)               | `24px` margin / `24px` pane gap |
| **Large**       | `1200px – 1599px` | Two-pane or spacious grid with max readable width            | Expanded Navigation Rail (`220px–256px`)                 | `24px` margin / `24px` pane gap |
| **Extra-Large** | `>= 1600px`       | Multi-pane / high-density grid with centered max-width rails | Expanded Navigation Rail                                 | `24px` margin / `24px` pane gap |

### Canonical Layouts for Proxy/Network Apps

- **List-Detail** (Settings, Profiles, Proxy Groups, Connections detail):
  - Compact: List view navigates to full-screen Detail view with back App Bar.
  - Expanded+: Persistent side-by-side List pane (`surface-container-low`) + Detail pane (`surface` or `surface-container-lowest`) with `16px–24px` visual separation.
- **Supporting Pane** (Dashboard, Topology/Traffic, Transform Chains):
  - Compact: Primary content full width; supporting inspectors/filters open in a modal Bottom Sheet.
  - Expanded+: Primary pane (`flex-1`) + fixed-width supporting pane (`320px–380px`).
- **Feed / Bento Grid** (Dashboard widgets, Proxy cards):
  - Use CSS Container Queries (`@container`) inside cards and widgets so every card adapts cleanly to its own allocated slot rather than global viewport queries.

---

## 3. Mobile Ergonomics & Cross-Input Readiness

Every redesigned view must work equally well with **touch**, **mouse/trackpad**, and **keyboard**:

1. **Touch targets:** Minimum `48x48px` interactive target on touch/compact viewports (expand hit area with transparent padding/pseudo-element if visual icon is `20px–24px`).
2. **No hover-only actions:** Every action revealed on hover (e.g., row copy, delete, edit, test latency) MUST remain reachable on touch devices via visible icon buttons, tap-to-open detail/sheet, or a trailing overflow menu button.
3. **Bottom-reach ergonomics on Compact:** Place primary navigation, floating action buttons, search/filter toolbars, and primary sheet confirmations in the lower thumb zone on `< 600px` viewports.
4. **Edge-to-edge safe areas:** Respect `env(safe-area-inset-top)`, `env(safe-area-inset-bottom)`, `env(safe-area-inset-left)`, and `env(safe-area-inset-right)` on root chrome, sticky headers, bottom bars, floating toolbars, and bottom sheets.
5. **No trapped horizontal overflow:** Toolbars and filter bars either collapse overflow into a menu or horizontally scroll with `overflow-x-auto` and fade/mask edges — never clip buttons off-screen or trigger page-level horizontal scroll.
6. **Desktop chrome isolation:** Custom titlebars, window drag regions (`data-tauri-drag-region`), and traffic-light / min-max-close controls render only on desktop shells via `@nyanpasu/platform`, leaving full edge-to-edge height on mobile/browser surfaces.
7. **Responsive tables & logs:** Data tables (Connections, Rules, Logs) must provide a compact stacked card/row layout (or prioritized columns + tap-for-detail bottom sheet) on `< 600px` viewports instead of forcing horizontal panning across 8 columns.

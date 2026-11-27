---
name: m3-expressive-ui
description: >-
  Design, redesign, audit, and implement user interfaces for Clash Nyanpasu using
  Google Material 3 Expressive (M3E), Android 17 surface translucency and adaptive
  design language, and modern Web/React/Tailwind v4/Radix/Motion primitives.
  Trigger whenever creating, refactoring, styling, or reviewing UI components,
  themes, motion, responsive/mobile layouts, or pages under frontend/.
---

# Material 3 Expressive & Android 17 UI Skill

Material 3 Expressive (M3E) is an **evolution of Material Design 3** — not a replacement ("not M4"). Backed by 46 research studies with 18,000+ participants ([Building with M3 Expressive](https://m3.material.io/blog/building-with-m3-expressive)), its headline finding is a **usability mechanism**, not mere decoration: users spot key UI elements **up to 4x faster** when expressive tactics guide attention.

Pair this skill with `.agents/skills/modern-web-guidance/SKILL.md` for modern web platform capabilities (container queries, CSS `@starting-style`, Popover/Anchor positioning, `color-mix`, View Transitions, accessibility, and responsive form factors).

## Source Contract

1. **Do not invent M3 numbers.** Every exact corner radius, type-scale measurement, spring damping/stiffness, elevation level, and component dimension comes from this skill's `references/` files.
2. **Separate M3 spec from product design decisions.** When a value is not defined by M3 (such as stagger delays, custom grid gaps, or desktop-specific chrome density), label it explicitly as a product design decision rather than attributing it to Material.
3. **Never confuse style with semantics.** Rounding corners, tinting surfaces, or adding blur does not make an interface M3E. Follow:
   `user goal -> information architecture -> hierarchy -> adaptive layout -> semantic tokens -> components -> states -> motion -> accessibility -> expressive focal points`.

## The Seven Expressive Tactics

Every expressive choice must answer: **does this direct attention to what matters on this screen?** If not, it is visual noise.

1. **Use a variety of shapes** — Mix round and square geometry for visual tension; break from the surrounding shape style to spotlight a key control. _Failure mode:_ small or timid shapes make essential actions look unimportant.
2. **Apply rich and nuanced colors** — Combine `primary`, `secondary`, and `tertiary` accents with the 5-level `surface-container-*` tonal hierarchy. _Failure mode:_ without tonal and role contrast, regions blend together.
3. **Guide attention with typography** — Pair the 15 baseline styles with the 15 **emphasized** styles (heavier weight, adjusted tracking) on headlines, primary actions, active selections, and key metrics.
4. **Contain content for emphasis** — Group related controls into deliberate tonal containers; give the primary task ample breathing room and the clearest surface separation. _Failure mode:_ ungrouped information blends together, while wrapping every row in its own card creates visual clutter.
5. **Add fluid and natural motion** — Use the spring physics system (**spatial** springs bounce; **effects** springs never overshoot) and state-driven shape morphing.
6. **Leverage component flexibility** — Adapt navigation, toolbars, button groups, and multi-pane layouts seamlessly across compact (mobile), medium (tablet/narrow desktop), and expanded/large (desktop) window size classes.
7. **Combine tactics for hero moments** — Limit each product to **one or two hero moments** (brief, focused, emotionally resonant highlights). Keep dense productivity screens (tables, logs, routine settings, forms) calm and scannable.

## Quick Reference

| Decision             | Standard Rule                                                                                                                                                                                                                                                                             |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Motion scheme**    | **Expressive** for primary interactions and hero moments; **Standard** for dense utilitarian surfaces.                                                                                                                                                                                    |
| **Spring type**      | Position / size / scale / rotation / radius -> **spatial** (overshoots). Color / opacity -> **effects** (`damping: 1.0`, never overshoots).                                                                                                                                               |
| **Spring speed**     | **Fast** = micro controls (switches, buttons, chips). **Default** = sheets, rails, cards, dialogs. **Slow** = full-screen / structural shifts.                                                                                                                                            |
| **Corner radius**    | 10-step scale: `0 / 4 / 8 / 12 / 16 / 20 / 28 / 32 / 48 / full`. Bold expressive additions: `20`, `32`, `48`.                                                                                                                                                                             |
| **Nested radii**     | Mandatory optical roundness: `innerRadius = outerRadius - padding`. Never reuse the parent radius on a padded child.                                                                                                                                                                      |
| **Surface layering** | Use tone-based `surface-container-lowest / low / default / high / highest`, not drop shadows or opacity hacks. Body = `surface`; Navigation = `surface-container`.                                                                                                                        |
| **Android 17 glass** | Reserve dynamic-color-tinted frosted translucency (`backdrop-filter`) for floating chrome overlays (floating toolbars, popovers, menus, dialogs, sheets, pill controls), never opaque reading panes. Always provide solid fallbacks for `prefers-reduced-transparency` and high contrast. |
| **Emphasized type**  | Opt in per element (primary buttons, badges, selected items, hero headings, KPI figures); never replace the entire baseline scale.                                                                                                                                                        |
| **Button hierarchy** | One high-emphasis CTA per view (`filled` or `FAB`/`SplitButton`). Use **Standard / Connected Button Groups** instead of legacy Segmented Buttons.                                                                                                                                         |
| **Navigation**       | Compact = Bottom Navigation Bar or Modal Nav Rail. Medium+ = Navigation Rail / Expanded Navigation Rail (replaces legacy Navigation Drawer). Never show two primary nav chrome bars at once.                                                                                              |
| **Wait feedback**    | `< 200ms`: none. `200ms - 5s`: morphing **Loading Indicator**. `> 5s`: **Progress Indicator** (indeterminate -> determinate). Never morph a loading indicator into a progress indicator.                                                                                                  |
| **Accessibility**    | Touch targets `>= 48x48px` (or `44x44px` minimum on dense desktop with `48px` touch hit area), text contrast `>= 4.5:1` (small) / `3:1` (large), non-color state cues, full keyboard focus rings, and `prefers-reduced-motion` support.                                                   |

## Progressive-Disclosure References

Load only the references needed for the current task:

| When working on                                                                                                                                                       | Read                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Color roles, `surfaceContainer*`, 10-step shape scale, 30-style typography, exact spring values & CSS bezier fallbacks                                                | [references/tokens.md](references/tokens.md)                                 |
| Buttons (10 types, XS-XL, morphing), Button Groups, Split Button, FABs, Toolbars, App Bars, Navigation, Loading/Progress, Cards, Lists, Inputs, Dialogs               | [references/components.md](references/components.md)                         |
| Android 17 frosted translucency, edge-to-edge safe areas, adaptive window size classes, canonical multi-pane layouts, mobile ergonomics                               | [references/android17-and-adaptive.md](references/android17-and-adaptive.md) |
| Clash Nyanpasu architecture mapping (`@nyanpasu/theme`, `@nyanpasu/ui`, Tailwind v4, Radix, `motion/react`, `data-slot`, `@nyanpasu/platform`, `modern-web-guidance`) | [references/nyanpasu-web-mapping.md](references/nyanpasu-web-mapping.md)     |

## Must Not (Anti-Patterns)

- **No fake Material:** Do not "Materialize" a screen by slapping `rounded-3xl`, pastel fills, or heavy blur onto flat desktop layouts without tonal hierarchy, motion physics, and semantic components.
- **No raw color literals:** Do not use `#hex`, `bg-white`, `bg-black`, `bg-zinc-*`, or arbitrary gray scales in UI components or pages when semantic M3 color roles apply.
- **No mismatched color pairs:** Always pair `X` with `on-X` (`primary` + `on-primary`, `primary-container` + `on-primary-container`, `surface-container-*` + `on-surface` / `on-surface-variant`).
- **No bouncing opacity/color:** Never apply a spatial spring (`damping < 1`) to opacity or color transitions.
- **No card soup:** Do not wrap every settings row or list item in an elevated card with drop shadows; use tonal grouping (`surface-container-low` / `surface-container`) with asymmetric grouped list radii or subtle `outline-variant` dividers.
- **No uniform loudness:** If every button is `filled`, every container is `48dp` rounded, and every surface is blurred glass, nothing has hierarchy.
- **No desktop-stretched mobile or cramped desktop:** Adapt layout structure, navigation chrome, touch targets, and action overflow across breakpoints rather than shrinking a desktop layout onto a phone.
- **No private re-implementations:** Build reusable interactive primitives in `@nyanpasu/ui` (wrapping Radix where applicable) rather than duplicating custom buttons, toggles, headers, or cards in page `_modules/`.

## Self-Audit Rubric (Mandatory Before Delivery)

Score every applicable dimension `0` (missing/incorrect), `1` (partial), or `2` (ready). Any `0` in **Component semantics**, **States & feedback**, or **Accessibility** blocks sign-off.

| Category                        | Verification Criteria                                                                                                                       |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| **Task clarity**                | Primary user goal and CTA are unmistakable within a glance.                                                                                 |
| **Information hierarchy**       | Surface container tones, emphasized typography, and grouping separate primary from secondary content cleanly.                               |
| **Component semantics**         | Controls match M3E roles (e.g., Connected Button Group vs Tabs vs Chips, Switch vs Checkbox vs Radio, Loading vs Progress).                 |
| **Token discipline**            | All colors, radii, typography, and springs resolve through semantic tokens; nested radii follow `outer - padding`.                          |
| **Android 17 cohesion**         | Floating overlays use restrained, dynamic-tinted translucency with crisp contrast and solid fallbacks; content panes remain calm.           |
| **Adaptive & mobile readiness** | Compact/Medium/Expanded breakpoints adapt navigation, panes, toolbars, safe areas, and `>= 48px` touch targets without horizontal overflow. |
| **States & feedback**           | Default, hover, focus-visible, pressed (shape morph), selected, disabled, loading, empty, and error states are all handled.                 |
| **Accessibility**               | Contrast ratios, keyboard navigation, ARIA semantics, reduced motion, and reduced transparency are verified.                                |
| **Expressive restraint**        | Expressive moments reinforce priority without adding noise to dense productivity workflows.                                                 |

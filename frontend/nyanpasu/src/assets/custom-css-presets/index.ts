import { m } from '@/paraglide/messages'
import type { CustomCssPreset } from '@/utils/custom-css-items'
import leftAlignedTitleBar from './left-aligned-title-bar.less?raw'
import materialYouSidebar from './material-you-sidebar.less?raw'
import verticalNavbar from './vertical-navbar.less?raw'

/**
 * Stylesheets bundled with the application, in the order they cascade.
 *
 * To add one, put a `<id>.less` file next to this one and register it:
 *
 * ```ts
 * import compactSidebar from './compact-sidebar.less?raw'
 *
 * {
 *   id: 'compact-sidebar',
 *   name: () => m.custom_css_preset_compact_sidebar_name(),
 *   description: () => m.custom_css_preset_compact_sidebar_description(),
 *   source: compactSidebar,
 * }
 * ```
 *
 * Presets are written against `data-slot` selectors and may use the same
 * slot shorthand as user CSS. The `id` is stored in the user's settings, so
 * never rename or reuse one.
 */
export const CUSTOM_CSS_PRESETS: readonly CustomCssPreset[] = [
  {
    id: 'vertical-navbar',
    name: () => m.custom_css_preset_vertical_navbar_name(),
    description: () => m.custom_css_preset_vertical_navbar_description(),
    source: verticalNavbar,
  },
  {
    id: 'material-you-sidebar',
    name: () => m.custom_css_preset_material_you_sidebar_name(),
    description: () => m.custom_css_preset_material_you_sidebar_description(),
    source: materialYouSidebar,
  },
  {
    id: 'left-aligned-title-bar',
    name: () => m.custom_css_preset_left_aligned_title_bar_name(),
    description: () => m.custom_css_preset_left_aligned_title_bar_description(),
    platform: 'macos',
    source: leftAlignedTitleBar,
  },
]

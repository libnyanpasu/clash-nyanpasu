import type { CustomCssPreset } from '@/utils/custom-css-items'

/**
 * Stylesheets bundled with the application, in the order they cascade.
 *
 * To add one, put a `<id>.less` file next to this one and register it:
 *
 * ```ts
 *  *
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
export const CUSTOM_CSS_PRESETS: readonly CustomCssPreset[] = []

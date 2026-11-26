import { DEFAULT_COLOR, type ThemeColorRole, type ThemeVariant } from './index'

export type SurfaceCategory =
  'chrome' | 'canvas' | 'card' | 'overlay' | 'input' | 'feedback'

export interface SurfaceLadderTier {
  id: string
  region: string
  category: SurfaceCategory
  lightRole: ThemeColorRole
  darkRole: ThemeColorRole
  foregroundRole: ThemeColorRole
  tailwindClasses: string
  usage: string
}

/**
 * Standardized Android 17 / M3E monotonic tonal surface ladder.
 * Keep in sync with `docs/development/design-language.md`.
 */
export const SURFACE_LADDER: readonly SurfaceLadderTier[] = [
  {
    id: 'window-header',
    region: 'Window titlebar (WindowHeader)',
    category: 'chrome',
    lightRole: 'surfaceContainerHighest',
    darkRole: 'surfaceContainerHighest',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-highest text-on-surface',
    usage:
      'Top-most desktop window chrome tier, clearly separated from navbar.',
  },
  {
    id: 'app-navbar',
    region: 'Top navigation bar (AppNavbar)',
    category: 'chrome',
    lightRole: 'surfaceContainerHigh',
    darkRole: 'surfaceContainer',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-high dark:bg-surface-container',
    usage: 'Primary horizontal navigation bar using secondary-container pills.',
  },
  {
    id: 'navigation-rail',
    region: 'Collapsible navigation rail (NavigationSidebar)',
    category: 'chrome',
    lightRole: 'surfaceContainer',
    darkRole: 'surfaceContainerLow',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container dark:bg-surface-container-low',
    usage:
      'One tone below AppNavbar to frame content without hairline dividers.',
  },
  {
    id: 'page-canvas',
    region: 'Page canvas & split-pane sidebars',
    category: 'canvas',
    lightRole: 'surfaceContainerLow',
    darkRole: 'surface',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-low dark:bg-surface',
    usage: 'Primary scrolling canvas behind cards and grouped lists.',
  },
  {
    id: 'card',
    region: 'Cards, grouped rows, tiles & inactive switches',
    category: 'card',
    lightRole: 'surfaceContainerLowest',
    darkRole: 'surfaceContainer',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-lowest dark:bg-surface-container',
    usage:
      'Pure-white lift on light canvas; tonal elevation above near-black dark canvas.',
  },
  {
    id: 'nested-well',
    region: 'Nested sub-cards / inner wells inside a card',
    category: 'card',
    lightRole: 'surfaceContainer',
    darkRole: 'surfaceContainerHigh',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container dark:bg-surface-container-high',
    usage:
      'Recessed or inset grouping inside a card; apply outer - padding radius.',
  },
  {
    id: 'side-pane',
    region: 'Tool-page side panes (Inspector / Transforms)',
    category: 'canvas',
    lightRole: 'surfaceContainerLow',
    darkRole: 'surfaceContainerLow',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-low',
    usage: 'Dedicated auxiliary inspector column beside dense data views.',
  },
  {
    id: 'elevated-overlay',
    region: 'Elevated popovers, menus, dialogs & raised cards',
    category: 'overlay',
    lightRole: 'surfaceContainerHigh',
    darkRole: 'surfaceContainerHigh',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-high',
    usage: 'Transient overlays and raised cards that sit above page content.',
  },
  {
    id: 'filled-control',
    region: 'Filled inputs, search fields, tracks & neutral badges',
    category: 'input',
    lightRole: 'surfaceContainerHighest',
    darkRole: 'surfaceContainerHighest',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-highest',
    usage: 'Interactive input fields, slider tracks, and subtle neutral chips.',
  },
  {
    id: 'code-ground',
    region: 'Code, YAML & diff editor ground',
    category: 'canvas',
    lightRole: 'surfaceContainerLowest',
    darkRole: 'surfaceContainerLowest',
    foregroundRole: 'onSurface',
    tailwindClasses: 'bg-surface-container-lowest',
    usage: 'Deepest high-contrast ground for syntax-highlighted code surfaces.',
  },
  {
    id: 'backdrop-scrim',
    region: 'Modal & drawer backdrop scrim',
    category: 'overlay',
    lightRole: 'scrim',
    darkRole: 'scrim',
    foregroundRole: 'inverseOnSurface',
    tailwindClasses: 'bg-scrim/32',
    usage: '32% scrim veil behind modals and drawers; never blurred.',
  },
  {
    id: 'tooltip',
    region: 'Tooltips & transient high-contrast feedback',
    category: 'feedback',
    lightRole: 'inverseSurface',
    darkRole: 'inverseSurface',
    foregroundRole: 'inverseOnSurface',
    tailwindClasses: 'bg-inverse-surface text-inverse-on-surface',
    usage: 'High-contrast ephemeral chrome such as tooltips and snackbars.',
  },
] as const

export interface ColorRoleGroup {
  id: string
  title: string
  description: string
  roles: readonly {
    role: ThemeColorRole
    onRole?: ThemeColorRole
    token: string
  }[]
}

export const COLOR_ROLE_GROUPS: readonly ColorRoleGroup[] = [
  {
    id: 'primary',
    title: 'Primary Accent',
    description:
      'Highest-emphasis CTAs, FABs, active toggles, and focus indicators.',
    roles: [
      { role: 'primary', onRole: 'onPrimary', token: 'primary' },
      { role: 'onPrimary', token: 'on-primary' },
      {
        role: 'primaryContainer',
        onRole: 'onPrimaryContainer',
        token: 'primary-container',
      },
      { role: 'onPrimaryContainer', token: 'on-primary-container' },
      { role: 'inversePrimary', token: 'inverse-primary' },
    ],
  },
  {
    id: 'secondary',
    title: 'Secondary Accent',
    description:
      'Active navigation pills, selected list rows, and tonal actions.',
    roles: [
      { role: 'secondary', onRole: 'onSecondary', token: 'secondary' },
      { role: 'onSecondary', token: 'on-secondary' },
      {
        role: 'secondaryContainer',
        onRole: 'onSecondaryContainer',
        token: 'secondary-container',
      },
      { role: 'onSecondaryContainer', token: 'on-secondary-container' },
    ],
  },
  {
    id: 'tertiary',
    title: 'Tertiary Accent',
    description:
      'Complementary focal highlights, callouts, and expressive badges.',
    roles: [
      { role: 'tertiary', onRole: 'onTertiary', token: 'tertiary' },
      { role: 'onTertiary', token: 'on-tertiary' },
      {
        role: 'tertiaryContainer',
        onRole: 'onTertiaryContainer',
        token: 'tertiary-container',
      },
      { role: 'onTertiaryContainer', token: 'on-tertiary-container' },
    ],
  },
  {
    id: 'error',
    title: 'Semantic Error',
    description:
      'Destructive actions and validation alerts across light and dark.',
    roles: [
      { role: 'error', onRole: 'onError', token: 'error' },
      { role: 'onError', token: 'on-error' },
      {
        role: 'errorContainer',
        onRole: 'onErrorContainer',
        token: 'error-container',
      },
      { role: 'onErrorContainer', token: 'on-error-container' },
    ],
  },
  {
    id: 'surfaces',
    title: 'Tonal Surface Hierarchy',
    description: 'Five-level container ladder plus surface dim/bright roles.',
    roles: [
      {
        role: 'surfaceContainerLowest',
        onRole: 'onSurface',
        token: 'surface-container-lowest',
      },
      {
        role: 'surfaceContainerLow',
        onRole: 'onSurface',
        token: 'surface-container-low',
      },
      {
        role: 'surfaceContainer',
        onRole: 'onSurface',
        token: 'surface-container',
      },
      {
        role: 'surfaceContainerHigh',
        onRole: 'onSurface',
        token: 'surface-container-high',
      },
      {
        role: 'surfaceContainerHighest',
        onRole: 'onSurface',
        token: 'surface-container-highest',
      },
      { role: 'surface', onRole: 'onSurface', token: 'surface' },
      { role: 'surfaceDim', onRole: 'onSurface', token: 'surface-dim' },
      { role: 'surfaceBright', onRole: 'onSurface', token: 'surface-bright' },
      { role: 'background', onRole: 'onBackground', token: 'background' },
    ],
  },
  {
    id: 'content-outline',
    title: 'Content, Outline & Utility',
    description:
      'Foreground text inks, interactive borders, dividers, and inverse chrome.',
    roles: [
      { role: 'onSurface', token: 'on-surface' },
      { role: 'onSurfaceVariant', token: 'on-surface-variant' },
      { role: 'onBackground', token: 'on-background' },
      {
        role: 'surfaceVariant',
        onRole: 'onSurfaceVariant',
        token: 'surface-variant',
      },
      { role: 'surfaceTint', token: 'surface-tint' },
      { role: 'outline', token: 'outline' },
      { role: 'outlineVariant', token: 'outline-variant' },
      {
        role: 'inverseSurface',
        onRole: 'inverseOnSurface',
        token: 'inverse-surface',
      },
      { role: 'inverseOnSurface', token: 'inverse-on-surface' },
      { role: 'shadow', token: 'shadow' },
      { role: 'scrim', token: 'scrim' },
    ],
  },
  {
    id: 'fixed',
    title: 'Fixed Accent Roles',
    description:
      'Mode-invariant accent tones for overlays and persistent badges.',
    roles: [
      {
        role: 'primaryFixed',
        onRole: 'onPrimaryFixed',
        token: 'primary-fixed',
      },
      {
        role: 'primaryFixedDim',
        onRole: 'onPrimaryFixedVariant',
        token: 'primary-fixed-dim',
      },
      { role: 'onPrimaryFixed', token: 'on-primary-fixed' },
      { role: 'onPrimaryFixedVariant', token: 'on-primary-fixed-variant' },
      {
        role: 'secondaryFixed',
        onRole: 'onSecondaryFixed',
        token: 'secondary-fixed',
      },
      {
        role: 'secondaryFixedDim',
        onRole: 'onSecondaryFixedVariant',
        token: 'secondary-fixed-dim',
      },
      { role: 'onSecondaryFixed', token: 'on-secondary-fixed' },
      { role: 'onSecondaryFixedVariant', token: 'on-secondary-fixed-variant' },
      {
        role: 'tertiaryFixed',
        onRole: 'onTertiaryFixed',
        token: 'tertiary-fixed',
      },
      {
        role: 'tertiaryFixedDim',
        onRole: 'onTertiaryFixedVariant',
        token: 'tertiary-fixed-dim',
      },
      { role: 'onTertiaryFixed', token: 'on-tertiary-fixed' },
      { role: 'onTertiaryFixedVariant', token: 'on-tertiary-fixed-variant' },
    ],
  },
] as const

export interface ContrastPairSpec {
  id: string
  label: string
  background: ThemeColorRole
  foreground: ThemeColorRole
  minRatio: number
}

/**
 * Canonical foreground/background pairings checked in the Gallery audit and tests.
 */
export const CONTRAST_PAIRS: readonly ContrastPairSpec[] = [
  {
    id: 'primary',
    label: 'primary / on-primary',
    background: 'primary',
    foreground: 'onPrimary',
    minRatio: 4.5,
  },
  {
    id: 'primary-container',
    label: 'primary-container / on-primary-container',
    background: 'primaryContainer',
    foreground: 'onPrimaryContainer',
    minRatio: 4.5,
  },
  {
    id: 'secondary',
    label: 'secondary / on-secondary',
    background: 'secondary',
    foreground: 'onSecondary',
    minRatio: 4.5,
  },
  {
    id: 'secondary-container',
    label: 'secondary-container / on-secondary-container',
    background: 'secondaryContainer',
    foreground: 'onSecondaryContainer',
    minRatio: 4.5,
  },
  {
    id: 'tertiary',
    label: 'tertiary / on-tertiary',
    background: 'tertiary',
    foreground: 'onTertiary',
    minRatio: 4.5,
  },
  {
    id: 'tertiary-container',
    label: 'tertiary-container / on-tertiary-container',
    background: 'tertiaryContainer',
    foreground: 'onTertiaryContainer',
    minRatio: 4.5,
  },
  {
    id: 'error',
    label: 'error / on-error',
    background: 'error',
    foreground: 'onError',
    minRatio: 4.5,
  },
  {
    id: 'error-container',
    label: 'error-container / on-error-container',
    background: 'errorContainer',
    foreground: 'onErrorContainer',
    minRatio: 4.5,
  },
  {
    id: 'surface',
    label: 'surface / on-surface',
    background: 'surface',
    foreground: 'onSurface',
    minRatio: 4.5,
  },
  {
    id: 'surface-variant-text',
    label: 'surface-container / on-surface-variant',
    background: 'surfaceContainer',
    foreground: 'onSurfaceVariant',
    minRatio: 4.5,
  },
  {
    id: 'surface-highest',
    label: 'surface-container-highest / on-surface',
    background: 'surfaceContainerHighest',
    foreground: 'onSurface',
    minRatio: 4.5,
  },
  {
    id: 'inverse-surface',
    label: 'inverse-surface / inverse-on-surface',
    background: 'inverseSurface',
    foreground: 'inverseOnSurface',
    minRatio: 4.5,
  },
] as const

export const FIXED_ICON_HUES = [
  {
    id: 'red',
    label: 'Red',
    bgClass: 'bg-red-container',
    textClass: 'text-on-red-container',
  },
  {
    id: 'orange',
    label: 'Orange',
    bgClass: 'bg-orange-container',
    textClass: 'text-on-orange-container',
  },
  {
    id: 'yellow',
    label: 'Yellow',
    bgClass: 'bg-yellow-container',
    textClass: 'text-on-yellow-container',
  },
  {
    id: 'green',
    label: 'Green',
    bgClass: 'bg-green-container',
    textClass: 'text-on-green-container',
  },
  {
    id: 'cyan',
    label: 'Cyan',
    bgClass: 'bg-cyan-container',
    textClass: 'text-on-cyan-container',
  },
  {
    id: 'blue',
    label: 'Blue',
    bgClass: 'bg-blue-container',
    textClass: 'text-on-blue-container',
  },
  {
    id: 'purple',
    label: 'Purple',
    bgClass: 'bg-purple-container',
    textClass: 'text-on-purple-container',
  },
  {
    id: 'pink',
    label: 'Pink',
    bgClass: 'bg-pink-container',
    textClass: 'text-on-pink-container',
  },
] as const

export interface ShapeTokenSpec {
  id: string
  px: number
  tailwindClass: string
  expressiveAddition: boolean
  usage: string
}

/**
 * Published 10-step Material 3 Expressive corner-radius scale.
 */
export const SHAPE_SCALE: readonly ShapeTokenSpec[] = [
  {
    id: 'none',
    px: 0,
    tailwindClass: 'rounded-none',
    expressiveAddition: false,
    usage: 'Full-bleed bars, sharp tables, code gutters.',
  },
  {
    id: 'extraSmall',
    px: 4,
    tailwindClass: 'rounded-xs',
    expressiveAddition: false,
    usage: 'Tight chips, grouped list inner corners.',
  },
  {
    id: 'small',
    px: 8,
    tailwindClass: 'rounded-sm',
    expressiveAddition: false,
    usage: 'Text fields, menus, pressed small buttons.',
  },
  {
    id: 'medium',
    px: 12,
    tailwindClass: 'rounded-md',
    expressiveAddition: false,
    usage: 'Standard cards, small FABs, compact tiles.',
  },
  {
    id: 'large',
    px: 16,
    tailwindClass: 'rounded-lg',
    expressiveAddition: false,
    usage: 'FABs, grouped settings sections, drawers.',
  },
  {
    id: 'largeIncreased',
    px: 20,
    tailwindClass: 'rounded-xl',
    expressiveAddition: true,
    usage: 'Expressive split panels and spacious tiles.',
  },
  {
    id: 'extraLarge',
    px: 28,
    tailwindClass: 'rounded-2xl',
    expressiveAddition: false,
    usage: 'Dialogs, bottom sheets, large FABs, hero cards.',
  },
  {
    id: 'extraLargeIncreased',
    px: 32,
    tailwindClass: 'rounded-3xl',
    expressiveAddition: true,
    usage: 'Expressive hero containers and floating modal sheets.',
  },
  {
    id: 'extraExtraLarge',
    px: 48,
    tailwindClass: 'rounded-[48px]',
    expressiveAddition: true,
    usage: 'Maximum expressive hero framing.',
  },
  {
    id: 'full',
    px: 9999,
    tailwindClass: 'rounded-full',
    expressiveAddition: false,
    usage: 'Buttons, pills, floating docks, switches, nav indicators.',
  },
] as const

export interface SpringSpec {
  id: string
  scheme: 'expressive' | 'standard'
  speed: 'fast' | 'default' | 'slow'
  kind: 'spatial' | 'effects'
  dampingRatio: number
  stiffness: number
  damping: number
  mass: number
  cssFallback: string
  cssDurationMs: number
  usage: string
}

/**
 * Published M3 Expressive spring physics converted for `motion/react` (`mass: 1`).
 */
export const M3E_MOTION_SPEC: readonly SpringSpec[] = [
  {
    id: 'expressive-fast-spatial',
    scheme: 'expressive',
    speed: 'fast',
    kind: 'spatial',
    dampingRatio: 0.6,
    stiffness: 800,
    damping: 33.9,
    mass: 1,
    cssFallback: 'cubic-bezier(0.42, 1.67, 0.21, 0.90)',
    cssDurationMs: 350,
    usage: 'Micro controls: button morphs, switch thumbs, chip toggles.',
  },
  {
    id: 'expressive-default-spatial',
    scheme: 'expressive',
    speed: 'default',
    kind: 'spatial',
    dampingRatio: 0.8,
    stiffness: 380,
    damping: 31.2,
    mass: 1,
    cssFallback: 'cubic-bezier(0.38, 1.21, 0.22, 1.00)',
    cssDurationMs: 500,
    usage: 'Partial-screen motion: sheets, dialogs, nav rail expand, cards.',
  },
  {
    id: 'expressive-slow-spatial',
    scheme: 'expressive',
    speed: 'slow',
    kind: 'spatial',
    dampingRatio: 0.8,
    stiffness: 200,
    damping: 22.6,
    mass: 1,
    cssFallback: 'cubic-bezier(0.39, 1.29, 0.35, 0.98)',
    cssDurationMs: 650,
    usage: 'Full-screen route or structural container transitions.',
  },
  {
    id: 'expressive-fast-effects',
    scheme: 'expressive',
    speed: 'fast',
    kind: 'effects',
    dampingRatio: 1.0,
    stiffness: 3800,
    damping: 123.3,
    mass: 1,
    cssFallback: 'cubic-bezier(0.31, 0.94, 0.34, 1.00)',
    cssDurationMs: 150,
    usage: 'Small color and opacity state-layer changes (never overshoots).',
  },
  {
    id: 'expressive-default-effects',
    scheme: 'expressive',
    speed: 'default',
    kind: 'effects',
    dampingRatio: 1.0,
    stiffness: 1600,
    damping: 80.0,
    mass: 1,
    cssFallback: 'cubic-bezier(0.34, 0.80, 0.34, 1.00)',
    cssDurationMs: 200,
    usage: 'Panel/dialog opacity, tint, and frosted blur fades.',
  },
  {
    id: 'expressive-slow-effects',
    scheme: 'expressive',
    speed: 'slow',
    kind: 'effects',
    dampingRatio: 1.0,
    stiffness: 800,
    damping: 56.6,
    mass: 1,
    cssFallback: 'cubic-bezier(0.34, 0.88, 0.34, 1.00)',
    cssDurationMs: 300,
    usage: 'Full-screen fades and surface refreshes.',
  },
  {
    id: 'standard-fast-spatial',
    scheme: 'standard',
    speed: 'fast',
    kind: 'spatial',
    dampingRatio: 0.9,
    stiffness: 1400,
    damping: 67.3,
    mass: 1,
    cssFallback: 'cubic-bezier(0.27, 1.06, 0.18, 1.00)',
    cssDurationMs: 350,
    usage: 'Utilitarian micro motion in dense tables, logs, and forms.',
  },
  {
    id: 'standard-default-spatial',
    scheme: 'standard',
    speed: 'default',
    kind: 'spatial',
    dampingRatio: 0.9,
    stiffness: 700,
    damping: 47.6,
    mass: 1,
    cssFallback: 'cubic-bezier(0.27, 1.06, 0.18, 1.00)',
    cssDurationMs: 500,
    usage: 'Utilitarian side panes, drawers, and inspectors.',
  },
  {
    id: 'standard-slow-spatial',
    scheme: 'standard',
    speed: 'slow',
    kind: 'spatial',
    dampingRatio: 0.9,
    stiffness: 300,
    damping: 31.2,
    mass: 1,
    cssFallback: 'cubic-bezier(0.27, 1.06, 0.18, 1.00)',
    cssDurationMs: 750,
    usage: 'Calm structural layout shifts.',
  },
] as const

export interface TypographyRoleSpec {
  role: string
  sizePx: number
  lineHeightPx: number
  baselineWeight: number
  baselineTracking: string
  emphasizedWeight: number
  emphasizedTracking: string
  usage: string
}

export const TYPOGRAPHY_SCALE: readonly TypographyRoleSpec[] = [
  {
    role: 'display-large',
    sizePx: 57,
    lineHeightPx: 64,
    baselineWeight: 400,
    baselineTracking: '-0.2px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Hero metrics and splash banners.',
  },
  {
    role: 'display-medium',
    sizePx: 45,
    lineHeightPx: 52,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Large dashboard hero figures.',
  },
  {
    role: 'display-small',
    sizePx: 36,
    lineHeightPx: 44,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Prominent numeric callouts.',
  },
  {
    role: 'headline-large',
    sizePx: 32,
    lineHeightPx: 40,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Expanded page headers.',
  },
  {
    role: 'headline-medium',
    sizePx: 28,
    lineHeightPx: 36,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Page titles and flexible expanded bars.',
  },
  {
    role: 'headline-small',
    sizePx: 24,
    lineHeightPx: 32,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Dialog headings and section heroes.',
  },
  {
    role: 'title-large',
    sizePx: 22,
    lineHeightPx: 28,
    baselineWeight: 400,
    baselineTracking: '0px',
    emphasizedWeight: 500,
    emphasizedTracking: '0px',
    usage: 'Collapsed app bar titles and sheet headers.',
  },
  {
    role: 'title-medium',
    sizePx: 16,
    lineHeightPx: 24,
    baselineWeight: 500,
    baselineTracking: '0.2px',
    emphasizedWeight: 700,
    emphasizedTracking: '0.15px',
    usage: 'Card titles and settings group headers.',
  },
  {
    role: 'title-small',
    sizePx: 14,
    lineHeightPx: 20,
    baselineWeight: 500,
    baselineTracking: '0.1px',
    emphasizedWeight: 700,
    emphasizedTracking: '0.1px',
    usage: 'Subheaders and active navigation items.',
  },
  {
    role: 'body-large',
    sizePx: 16,
    lineHeightPx: 24,
    baselineWeight: 400,
    baselineTracking: '0.5px',
    emphasizedWeight: 500,
    emphasizedTracking: '0.15px',
    usage: 'Primary list item titles and readable copy.',
  },
  {
    role: 'body-medium',
    sizePx: 14,
    lineHeightPx: 20,
    baselineWeight: 400,
    baselineTracking: '0.2px',
    emphasizedWeight: 500,
    emphasizedTracking: '0.25px',
    usage: 'Standard UI copy, descriptions, and table cells.',
  },
  {
    role: 'body-small',
    sizePx: 12,
    lineHeightPx: 16,
    baselineWeight: 400,
    baselineTracking: '0.4px',
    emphasizedWeight: 500,
    emphasizedTracking: '0.4px',
    usage: 'Supporting copy, timestamps, and metadata.',
  },
  {
    role: 'label-large',
    sizePx: 14,
    lineHeightPx: 20,
    baselineWeight: 500,
    baselineTracking: '0.1px',
    emphasizedWeight: 700,
    emphasizedTracking: '0.1px',
    usage: 'Buttons, tabs, navigation labels, prominent chips.',
  },
  {
    role: 'label-medium',
    sizePx: 12,
    lineHeightPx: 16,
    baselineWeight: 500,
    baselineTracking: '0.5px',
    emphasizedWeight: 700,
    emphasizedTracking: '0.5px',
    usage: 'Compact buttons, nav destinations, and badges.',
  },
  {
    role: 'label-small',
    sizePx: 11,
    lineHeightPx: 16,
    baselineWeight: 500,
    baselineTracking: '0.5px',
    emphasizedWeight: 700,
    emphasizedTracking: '0.5px',
    usage: 'Micro badges, dense headers, and keyboard hints.',
  },
] as const

export const THEME_VARIANT_SPECS: readonly {
  id: ThemeVariant
  label: string
  summary: string
}[] = [
  {
    id: 'tonalSpot',
    label: 'Tonal Spot',
    summary: 'Default calm neutral surfaces faithful to the user seed.',
  },
  {
    id: 'expressive',
    label: 'Expressive',
    summary: 'Detached accent hues for playful high-energy surfaces.',
  },
  {
    id: 'vibrant',
    label: 'Vibrant',
    summary: 'Max-chroma primary with energetic secondary and tertiary hues.',
  },
  {
    id: 'fidelity',
    label: 'Fidelity',
    summary:
      'Keeps primary close to the exact source color even at high chroma.',
  },
  {
    id: 'content',
    label: 'Content',
    summary: 'Source-driven palette tailored to media or artwork seeds.',
  },
  {
    id: 'neutral',
    label: 'Neutral',
    summary: 'Near-grayscale restraint with whisper-quiet tinting.',
  },
  {
    id: 'monochrome',
    label: 'Monochrome',
    summary: 'Pure achromatic grayscale ladder for maximum clarity.',
  },
  {
    id: 'rainbow',
    label: 'Rainbow',
    summary: 'Chromatic accents over purely neutral surfaces.',
  },
  {
    id: 'fruitSalad',
    label: 'Fruit Salad',
    summary: 'Playful contrasting accent rotation across roles.',
  },
] as const

export const THEME_PRESET_SEEDS = [
  { name: 'Nyanpasu Indigo', hex: DEFAULT_COLOR },
  { name: 'Sakura Rose', hex: '#c84b7a' },
  { name: 'Forest Moss', hex: '#2f7d57' },
  { name: 'Terracotta Clay', hex: '#c65d3b' },
  { name: 'Deep Violet', hex: '#6f52c7' },
  { name: 'Ocean Teal', hex: '#007c8a' },
  { name: 'Amber Ochre', hex: '#b67617' },
  { name: 'Graphite Slate', hex: '#5c6472' },
] as const

import { kebabCase } from 'es-toolkit'
import {
  argbFromHex,
  Blend,
  Contrast,
  Hct,
  hexFromArgb,
  SchemeContent,
  SchemeExpressive,
  SchemeFidelity,
  SchemeFruitSalad,
  SchemeMonochrome,
  SchemeNeutral,
  SchemeRainbow,
  SchemeTonalSpot,
  SchemeVibrant,
  type DynamicScheme,
} from '@material/material-color-utilities'

export { hexFromArgb } from '@material/material-color-utilities'

export function getHctTone(argb: number): number {
  return Hct.fromInt(argb).tone
}

export function getContrastRatio(argbA: number, argbB: number): number {
  return Contrast.ratioOfTones(getHctTone(argbA), getHctTone(argbB))
}

export const DEFAULT_COLOR = '#3e64a7'

export type ThemeVariant =
  | 'tonalSpot'
  | 'expressive'
  | 'fidelity'
  | 'content'
  | 'neutral'
  | 'monochrome'
  | 'vibrant'
  | 'rainbow'
  | 'fruitSalad'

export type ThemeSpecVersion = '2025' | '2021'

export type CreateThemeOptions = {
  variant?: ThemeVariant
  contrastLevel?: number
  specVersion?: ThemeSpecVersion
}

export const DEFAULT_THEME_VARIANT: ThemeVariant = 'tonalSpot'
export const DEFAULT_THEME_SPEC_VERSION: ThemeSpecVersion = '2025'
export const DEFAULT_CONTRAST_LEVEL = 0

export const THEME_COLOR_ROLES = [
  'primary',
  'onPrimary',
  'primaryContainer',
  'onPrimaryContainer',
  'secondary',
  'onSecondary',
  'secondaryContainer',
  'onSecondaryContainer',
  'tertiary',
  'onTertiary',
  'tertiaryContainer',
  'onTertiaryContainer',
  'error',
  'onError',
  'errorContainer',
  'onErrorContainer',
  'background',
  'onBackground',
  'surface',
  'onSurface',
  'surfaceDim',
  'surfaceBright',
  'surfaceContainerLowest',
  'surfaceContainerLow',
  'surfaceContainer',
  'surfaceContainerHigh',
  'surfaceContainerHighest',
  'surfaceVariant',
  'onSurfaceVariant',
  'surfaceTint',
  'outline',
  'outlineVariant',
  'shadow',
  'scrim',
  'inverseSurface',
  'inverseOnSurface',
  'inversePrimary',
  'primaryFixed',
  'primaryFixedDim',
  'onPrimaryFixed',
  'onPrimaryFixedVariant',
  'secondaryFixed',
  'secondaryFixedDim',
  'onSecondaryFixed',
  'onSecondaryFixedVariant',
  'tertiaryFixed',
  'tertiaryFixedDim',
  'onTertiaryFixed',
  'onTertiaryFixedVariant',
] as const

export type ThemeColorRole = (typeof THEME_COLOR_ROLES)[number]
export type ThemeScheme = Record<ThemeColorRole, number>

export type Theme = {
  source: number
  schemes: {
    light: ThemeScheme
    dark: ThemeScheme
  }
}

const SCHEME_CONSTRUCTORS: Record<ThemeVariant, typeof SchemeTonalSpot> = {
  tonalSpot: SchemeTonalSpot,
  expressive: SchemeExpressive,
  fidelity: SchemeFidelity,
  content: SchemeContent,
  neutral: SchemeNeutral,
  monochrome: SchemeMonochrome,
  vibrant: SchemeVibrant,
  rainbow: SchemeRainbow,
  fruitSalad: SchemeFruitSalad,
}

function extractScheme(scheme: DynamicScheme): ThemeScheme {
  const result = {} as ThemeScheme

  for (const role of THEME_COLOR_ROLES) {
    result[role] = scheme[role]
  }

  return result
}

export function isValidThemePalette(value: unknown): value is Theme {
  if (!value || typeof value !== 'object') {
    return false
  }

  const candidate = value as Partial<Theme>
  if (typeof candidate.source !== 'number' || !candidate.schemes) {
    return false
  }

  for (const mode of ['light', 'dark'] as const) {
    const scheme = candidate.schemes[mode] as
      Partial<Record<ThemeColorRole, unknown>> | undefined
    if (!scheme || typeof scheme !== 'object') {
      return false
    }

    for (const role of THEME_COLOR_ROLES) {
      if (typeof scheme[role] !== 'number') {
        return false
      }
    }
  }

  return true
}

export enum ThemeMode {
  LIGHT = 'light',
  DARK = 'dark',
  SYSTEM = 'system',
}

export type ResolvedThemeMode = ThemeMode.LIGHT | ThemeMode.DARK

export const alpha = (color: string, value: number) => {
  return `color-mix(in srgb, ${color} ${(value * 100).toFixed(2)}%, transparent ${((1 - value) * 100).toFixed(2)}%)`
}

export const lighten = (color: string, value: number) => {
  return `color-mix(in lch, ${color} ${((1 - value) * 100).toFixed(2)}%, white ${(value * 100).toFixed(2)}%)`
}

export const darken = (color: string, value: number) => {
  return `color-mix(in lch, ${color} ${((1 - value) * 100).toFixed(2)}%, black ${(value * 100).toFixed(2)}%)`
}

/** The prefix of the CSS variables that carry the syntax colors. */
export const SYNTAX_CSS_VARIABLE_PREFIX = '--syntax-'

// Hues the Material roles lack, pulled toward the theme's source color.
const SYNTAX_GREEN = '#4caf50'
const SYNTAX_ORANGE = '#ff9800'
const SYNTAX_MAX_CHROMA = 48
// The tones of the `primary` role, so every syntax color reads alike.
const SYNTAX_TONES = { light: 40, dark: 80 } as const

/**
 * The colors code is highlighted in. Every highlighter draws from them, so an
 * editor and a read-only code block show the same token in the same color.
 */
export type SyntaxColors = {
  foreground: string
  comment: string
  keyword: string
  function: string
  constant: string
  string: string
  stringExpression: string
  parameter: string
  punctuation: string
  link: string
  inserted: string
  deleted: string
  changed: string
}

export function getSyntaxColors(
  theme: Theme,
  mode: ResolvedThemeMode,
): SyntaxColors {
  const scheme = getThemeScheme(theme, mode)
  const harmonized = (color: string) => {
    const { hue, chroma } = Hct.fromInt(
      Blend.harmonize(argbFromHex(color), theme.source),
    )

    return hexFromArgb(
      Hct.from(
        hue,
        Math.min(chroma, SYNTAX_MAX_CHROMA),
        SYNTAX_TONES[mode],
      ).toInt(),
    )
  }

  const green = harmonized(SYNTAX_GREEN)
  const orange = harmonized(SYNTAX_ORANGE)

  return {
    foreground: hexFromArgb(scheme.onSurface),
    comment: hexFromArgb(scheme.outline),
    keyword: hexFromArgb(scheme.primary),
    function: hexFromArgb(scheme.tertiary),
    constant: orange,
    string: green,
    stringExpression: green,
    parameter: hexFromArgb(scheme.secondary),
    punctuation: hexFromArgb(scheme.onSurfaceVariant),
    link: hexFromArgb(scheme.primary),
    inserted: green,
    deleted: hexFromArgb(scheme.error),
    changed: orange,
  }
}

const syntaxCssVars = (theme: Theme, mode: ResolvedThemeMode) =>
  Object.entries(getSyntaxColors(theme, mode))
    .map(([key, color]) => {
      const name = key === 'foreground' ? key : `token-${kebabCase(key)}`

      return `${SYNTAX_CSS_VARIABLE_PREFIX}${name}: ${color};`
    })
    .join('')

export function generateThemeCssVarsForMode(
  theme: Theme,
  mode: ResolvedThemeMode,
): string {
  const scheme = getThemeScheme(theme, mode)
  let vars = ''

  for (const [key, value] of Object.entries(scheme)) {
    vars += `--color-md-${kebabCase(key)}: ${hexFromArgb(value)};`
  }

  vars += `${SYNTAX_CSS_VARIABLE_PREFIX}background: transparent;`
  vars += syntaxCssVars(theme, mode)

  return vars
}

export function generateScopedThemeCssVars(
  theme: Theme,
  scopeSelector: string,
  mode: ResolvedThemeMode,
): string {
  const scheme = getThemeScheme(theme, mode)
  let vars = generateThemeCssVarsForMode(theme, mode)

  // Tailwind v4 `@theme` resolves `--color-*: var(--color-md-*)` on `:root`,
  // so scoped light/dark preview subtrees must also re-bind `--color-*`
  // directly to prevent inheriting the root mode's computed values.
  for (const [key, value] of Object.entries(scheme)) {
    vars += `--color-${kebabCase(key)}: ${hexFromArgb(value)};`
  }

  vars +=
    mode === ThemeMode.DARK
      ? '--color-tc: #8e9099;--color-ok: #7bdc95;--color-warn: #ffb870;'
      : '--color-tc: #74777f;--color-ok: #1b6d3b;--color-warn: #8a5100;'

  return `${scopeSelector}{${vars}}`
}

export function generateThemeCssVars(theme: Theme): string {
  const lightCssVars = `:root{${generateThemeCssVarsForMode(theme, ThemeMode.LIGHT)}}`
  const darkCssVars = `:root.dark{${generateThemeCssVarsForMode(theme, ThemeMode.DARK)}}`

  return lightCssVars + darkCssVars
}

export function createTheme(color: string, options: CreateThemeOptions = {}) {
  const {
    variant = DEFAULT_THEME_VARIANT,
    contrastLevel = DEFAULT_CONTRAST_LEVEL,
    specVersion = DEFAULT_THEME_SPEC_VERSION,
  } = options
  const source = argbFromHex(color || DEFAULT_COLOR)
  const sourceHct = Hct.fromInt(source)
  const SchemeCtor = SCHEME_CONSTRUCTORS[variant]

  const palette: Theme = {
    source,
    schemes: {
      light: extractScheme(
        new SchemeCtor(sourceHct, false, contrastLevel, specVersion),
      ),
      dark: extractScheme(
        new SchemeCtor(sourceHct, true, contrastLevel, specVersion),
      ),
    },
  }

  return {
    palette,
    cssVars: generateThemeCssVars(palette),
  }
}

export function getThemeScheme(
  theme: Theme,
  mode: ResolvedThemeMode,
): ThemeScheme {
  const scheme = theme.schemes[mode] as ThemeScheme & {
    toJSON?: () => ThemeScheme
  }

  return typeof scheme.toJSON === 'function' ? scheme.toJSON() : scheme
}

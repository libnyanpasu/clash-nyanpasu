import { kebabCase } from 'es-toolkit'
import {
  argbFromHex,
  Blend,
  Hct,
  hexFromArgb,
  themeFromSourceColor,
  type Theme,
} from '@material/material-color-utilities'

export { hexFromArgb } from '@material/material-color-utilities'
export type { Theme } from '@material/material-color-utilities'

export const DEFAULT_COLOR = '#3e64a7'

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

export function generateThemeCssVars(theme: Theme): string {
  const { schemes } = theme
  let lightCssVars = ':root{'
  let darkCssVars = ':root.dark{'

  Object.entries(schemes).forEach(([mode, scheme]) => {
    const inputScheme =
      typeof scheme.toJSON === 'function' ? scheme.toJSON() : scheme

    Object.entries(inputScheme).forEach(([key, value]) => {
      if (mode === 'light') {
        lightCssVars += `--color-md-${kebabCase(key)}: ${hexFromArgb(value)};`
      } else {
        darkCssVars += `--color-md-${kebabCase(key)}: ${hexFromArgb(value)};`
      }
    })
  })

  lightCssVars += `${SYNTAX_CSS_VARIABLE_PREFIX}background: transparent;`
  lightCssVars += syntaxCssVars(theme, ThemeMode.LIGHT)
  darkCssVars += syntaxCssVars(theme, ThemeMode.DARK)

  lightCssVars += '}'
  darkCssVars += '}'

  return lightCssVars + darkCssVars
}

export function createTheme(color: string) {
  const theme = themeFromSourceColor(argbFromHex(color || DEFAULT_COLOR))

  return {
    palette: JSON.parse(JSON.stringify(theme)) as Theme,
    cssVars: generateThemeCssVars(theme),
  }
}

export function getThemeScheme(theme: Theme, mode: ResolvedThemeMode) {
  const scheme = theme.schemes[mode]

  return typeof scheme.toJSON === 'function' ? scheme.toJSON() : scheme
}

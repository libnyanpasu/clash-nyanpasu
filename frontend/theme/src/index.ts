import { kebabCase } from 'es-toolkit'
import {
  argbFromHex,
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

export function generateThemeCssVars({ schemes }: Theme): string {
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

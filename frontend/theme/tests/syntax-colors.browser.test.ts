import { expect, test } from 'vitest'
import {
  createTheme,
  getSyntaxColors,
  getThemeScheme,
  isValidThemePalette,
  SYNTAX_CSS_VARIABLE_PREFIX,
  THEME_COLOR_ROLES,
  ThemeMode,
} from '../src'

const HEX_COLOR = /^#[0-9a-f]{6}$/

test('every syntax color is a hex color in both modes', () => {
  const { palette } = createTheme('#3e64a7')

  for (const mode of [ThemeMode.LIGHT, ThemeMode.DARK] as const) {
    for (const color of Object.values(getSyntaxColors(palette, mode))) {
      expect(color).toMatch(HEX_COLOR)
    }
  }
})

test('the syntax colors follow the theme color and the mode', () => {
  const blue = createTheme('#3e64a7').palette
  const red = createTheme('#a73e3e').palette

  expect(getSyntaxColors(blue, ThemeMode.DARK).keyword).not.toBe(
    getSyntaxColors(red, ThemeMode.DARK).keyword,
  )
  expect(getSyntaxColors(blue, ThemeMode.DARK).string).not.toBe(
    getSyntaxColors(blue, ThemeMode.LIGHT).string,
  )
})

test('the theme CSS carries each mode its syntax colors', () => {
  const { palette, cssVars } = createTheme('#3e64a7')
  const [light, dark] = cssVars.split(':root.dark{')

  expect(light).toContain(
    `${SYNTAX_CSS_VARIABLE_PREFIX}token-keyword: ${getSyntaxColors(palette, ThemeMode.LIGHT).keyword};`,
  )
  expect(dark).toContain(
    `${SYNTAX_CSS_VARIABLE_PREFIX}token-string-expression: ${getSyntaxColors(palette, ThemeMode.DARK).stringExpression};`,
  )
  expect(dark).toContain(
    `${SYNTAX_CSS_VARIABLE_PREFIX}foreground: ${getSyntaxColors(palette, ThemeMode.DARK).foreground};`,
  )
})

test('createTheme emits all M3E surface container and fixed color roles in both modes', () => {
  const { palette, cssVars } = createTheme('#3e64a7')
  const [lightCss, darkCss] = cssVars.split(':root.dark{')

  expect(isValidThemePalette(palette)).toBe(true)

  for (const mode of [ThemeMode.LIGHT, ThemeMode.DARK] as const) {
    const scheme = getThemeScheme(palette, mode)
    for (const role of THEME_COLOR_ROLES) {
      expect(typeof scheme[role]).toBe('number')
    }
  }

  for (const token of [
    '--color-md-surface-dim:',
    '--color-md-surface-bright:',
    '--color-md-surface-container-lowest:',
    '--color-md-surface-container-low:',
    '--color-md-surface-container:',
    '--color-md-surface-container-high:',
    '--color-md-surface-container-highest:',
    '--color-md-primary-fixed:',
    '--color-md-primary-fixed-dim:',
    '--color-md-on-primary-fixed:',
    '--color-md-on-primary-fixed-variant:',
    '--color-md-secondary-fixed:',
    '--color-md-tertiary-fixed:',
  ]) {
    expect(lightCss).toContain(token)
    expect(darkCss).toContain(token)
  }
})

test('createTheme supports variant and contrastLevel options', () => {
  const tonal = createTheme('#3e64a7')
  const expressive = createTheme('#3e64a7', { variant: 'expressive' })
  const highContrast = createTheme('#3e64a7', { contrastLevel: 1 })

  expect(expressive.palette.schemes.light.tertiary).not.toBe(
    tonal.palette.schemes.light.tertiary,
  )
  expect(highContrast.palette.schemes.light.primary).not.toBe(
    tonal.palette.schemes.light.primary,
  )
})

test('isValidThemePalette rejects legacy v1 palettes missing surface container roles', () => {
  expect(isValidThemePalette(null)).toBe(false)
  expect(isValidThemePalette({})).toBe(false)
  expect(
    isValidThemePalette({
      source: 0xff3e64a7,
      schemes: {
        light: { primary: 0xff005db5, surface: 0xfffdfbff },
        dark: { primary: 0xffa8c8ff, surface: 0xff1a1b1e },
      },
    }),
  ).toBe(false)
})

import { expect, test } from 'vitest'
import {
  createTheme,
  getSyntaxColors,
  SYNTAX_CSS_VARIABLE_PREFIX,
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

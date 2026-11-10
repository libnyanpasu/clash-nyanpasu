import { expect, test } from 'vitest'
import { createTheme, getSyntaxColors, ThemeMode } from '@nyanpasu/theme'
import {
  createMonacoTheme,
  monacoThemeName,
} from '../src/pages/(editor)/editor/_modules/monaco-theme'

const { palette } = createTheme('#3e64a7')

test('each mode has its own theme on the matching Monaco base', () => {
  expect(monacoThemeName(ThemeMode.LIGHT)).not.toBe(
    monacoThemeName(ThemeMode.DARK),
  )
  expect(createMonacoTheme(palette, ThemeMode.LIGHT).base).toBe('vs')
  expect(createMonacoTheme(palette, ThemeMode.DARK).base).toBe('vs-dark')
})

test('tokens take the shared syntax colors', () => {
  const syntax = getSyntaxColors(palette, ThemeMode.DARK)
  const { rules } = createMonacoTheme(palette, ThemeMode.DARK)
  const colorOf = (token: string) =>
    rules.find((rule) => rule.token === token)?.foreground

  expect(colorOf('comment')).toBe(syntax.comment.slice(1))
  expect(colorOf('string')).toBe(syntax.stringExpression.slice(1))
  // A YAML key is a keyword, as the read-only viewers highlight it.
  expect(colorOf('type.yaml')).toBe(syntax.keyword.slice(1))
})

test('every editor color is a hex color Monaco accepts', () => {
  for (const mode of [ThemeMode.LIGHT, ThemeMode.DARK] as const) {
    for (const color of Object.values(
      createMonacoTheme(palette, mode).colors,
    )) {
      expect(color).toMatch(/^#[0-9a-f]{6}([0-9a-f]{2})?$/)
    }
  }
})

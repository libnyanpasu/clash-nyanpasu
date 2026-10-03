import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { createTheme, ThemeMode } from '@nyanpasu/theme'
import {
  ExperimentalThemeProvider,
  useExperimentalThemeContext,
} from '../src/components/providers/theme-provider'

const settings = vi.hoisted(() => ({
  theme_mode: 'light' as string | undefined,
  theme_color: '#ff0000' as string | undefined,
}))
const upsert = vi.hoisted(() => vi.fn(async () => {}))
// Like the real hook, every call returns a new object.
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: keyof typeof settings) => ({
    value: settings[key],
    upsert,
  }),
}))
const themes = vi.hoisted(() => ({ created: 0 }))
vi.mock('@material/material-color-utilities', async (importOriginal) => {
  const original =
    await importOriginal<typeof import('@material/material-color-utilities')>()
  return {
    ...original,
    themeFromSourceColor: (
      ...args: Parameters<typeof original.themeFromSourceColor>
    ) => {
      themes.created += 1
      return original.themeFromSourceColor(...args)
    },
  }
})

const PALETTE_KEY = 'theme-palette-v1'
const CSS_VARS_KEY = 'theme-css-vars-v1'

let rerender: () => void = () => {}
let palette: unknown
function Harness() {
  const [, setTick] = useState(0)
  rerender = () => setTick((tick) => tick + 1)
  return (
    <ExperimentalThemeProvider>
      <Reader />
    </ExperimentalThemeProvider>
  )
}
let context: ReturnType<typeof useExperimentalThemeContext> | undefined
function Reader() {
  context = useExperimentalThemeContext()
  palette = context.themePalette
  return null
}

const customStyle = () => document.getElementById('custom-theme')?.innerHTML

test('re-rendering with the same theme color rebuilds and stores nothing', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  await expect.poll(() => customStyle()).toBeTruthy()
  const created = themes.created
  const setItem = vi.spyOn(Storage.prototype, 'setItem')
  onTestFinished(() => setItem.mockRestore())

  for (let i = 0; i < 10; i++) {
    await view.rerender(<Harness />)
    rerender()
  }
  await new Promise((resolve) => setTimeout(resolve, 50))

  expect(themes.created - created).toBe(0)
  expect(setItem).not.toHaveBeenCalled()
  // Readers receive the plain palette the cache holds.
  const light = (palette as { schemes: { light: object } }).schemes.light
  expect(Object.values(light).every((value) => typeof value === 'number')).toBe(
    true,
  )
})

test('the cached theme paints until the theme color loads', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  settings.theme_color = '#00ff00'
  const first = await render(<Harness />)
  await expect.poll(() => customStyle()).toBeTruthy()
  const green = customStyle()
  await first.unmount()
  expect(localStorage.getItem(CSS_VARS_KEY)).toBe(JSON.stringify(green))
  expect(localStorage.getItem(PALETTE_KEY)).toBeTruthy()

  // The next launch, before the settings have loaded.
  settings.theme_color = undefined
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(customStyle()).toBe(green)

  settings.theme_color = '#0000ff'
  rerender()
  await expect.poll(() => customStyle()).not.toBe(green)
  expect(localStorage.getItem(CSS_VARS_KEY)).toBe(JSON.stringify(customStyle()))
})

test('a color preview repaints without saving or caching it', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  upsert.mockClear()
  settings.theme_mode = 'light'
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  const saved = createTheme('#ff0000').cssVars
  const preview = createTheme('#0000ff').cssVars
  await expect.poll(() => customStyle()).toBe(saved)
  await expect
    .poll(() => localStorage.getItem(CSS_VARS_KEY))
    .toBe(JSON.stringify(saved))
  const setItem = vi.spyOn(Storage.prototype, 'setItem')
  onTestFinished(() => setItem.mockRestore())

  context!.setThemePreview({ color: '#0000ff' })

  await expect.poll(() => customStyle()).toBe(preview)
  expect(context!.themeColor).toBe('#ff0000')
  expect(setItem).not.toHaveBeenCalled()

  context!.setThemePreview(null)

  await expect.poll(() => customStyle()).toBe(saved)
  // The frame that still paints the deferred preview color is not cached.
  await expect
    .poll(() => localStorage.getItem(CSS_VARS_KEY))
    .toBe(JSON.stringify(saved))
  expect(
    setItem.mock.calls.some(([, value]) => value === JSON.stringify(preview)),
  ).toBe(false)
  expect(upsert).not.toHaveBeenCalled()
})

test('a mode preview switches the color scheme without saving it', async ({
  onTestFinished,
}) => {
  upsert.mockClear()
  settings.theme_mode = 'light'
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  const root = document.documentElement
  await expect.poll(() => root.classList.contains('light')).toBe(true)

  context!.setThemePreview({ mode: ThemeMode.DARK })

  await expect.poll(() => root.classList.contains('dark')).toBe(true)
  expect(root.classList.contains('light')).toBe(false)
  expect(context!.themeMode).toBe(ThemeMode.LIGHT)
  expect(context!.currentThemeMode).toBe(ThemeMode.DARK)

  context!.setThemePreview(null)

  await expect.poll(() => root.classList.contains('light')).toBe(true)
  expect(root.classList.contains('dark')).toBe(false)
  expect(upsert).not.toHaveBeenCalled()
})

import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { createTheme } from '@nyanpasu/theme'
import {
  ExperimentalThemeProvider,
  THEME_CSS_VARS_KEY,
  THEME_PALETTE_KEY,
  useExperimentalThemeContext,
} from '../src/components/providers/theme-provider'

const settings = vi.hoisted(() => ({
  theme_mode: 'light' as string | undefined,
  theme_color: '#ff0000' as string | undefined,
  theme_variant: undefined as string | undefined,
  theme_contrast_level: undefined as number | undefined,
  theme_spec_version: undefined as string | undefined,
}))
const upsert = vi.hoisted(() => vi.fn(async () => {}))
const patch = vi.hoisted(() =>
  vi.fn<(patch: object) => Promise<void>>(async () => {}),
)
// Like the real hook, every call returns a new object.
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: keyof typeof settings) => ({
    value: settings[key],
    upsert,
  }),
  useSettings: () => ({ upsert: { mutateAsync: patch } }),
}))
const themes = vi.hoisted(() => ({ created: 0 }))
vi.mock('@nyanpasu/theme', async (importOriginal) => {
  const original = await importOriginal<typeof import('@nyanpasu/theme')>()
  return {
    ...original,
    createTheme: (...args: Parameters<typeof original.createTheme>) => {
      themes.created += 1
      return original.createTheme(...args)
    },
  }
})

const PALETTE_KEY = THEME_PALETTE_KEY
const CSS_VARS_KEY = THEME_CSS_VARS_KEY

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

test('the theme follows every saved option', async ({ onTestFinished }) => {
  settings.theme_color = '#ff0000'
  settings.theme_variant = 'fruit_salad'
  settings.theme_contrast_level = 1
  settings.theme_spec_version = '2021'
  onTestFinished(() => {
    settings.theme_variant = undefined
    settings.theme_contrast_level = undefined
    settings.theme_spec_version = undefined
  })
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())

  await expect
    .poll(() => customStyle())
    .toBe(
      createTheme('#ff0000', {
        variant: 'fruitSalad',
        contrastLevel: 1,
        specVersion: '2021',
      }).cssVars,
    )
  expect(context!.themeOptions).toEqual({
    color: '#ff0000',
    variant: 'fruitSalad',
    contrastLevel: 1,
    specVersion: '2021',
  })
})

test('setThemeOptions saves only the changed options in one patch', async ({
  onTestFinished,
}) => {
  patch.mockClear()
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  await expect.poll(() => customStyle()).toBeTruthy()

  await context!.setThemeOptions({
    ...context!.themeOptions,
    color: '#FF0000',
    variant: 'tonalSpot',
  })
  expect(patch).not.toHaveBeenCalled()

  await context!.setThemeOptions({
    color: '#00ff00',
    variant: 'fruitSalad',
    contrastLevel: 0,
    specVersion: '2025',
  })
  expect(patch.mock.calls).toEqual([
    [{ theme_color: '#00ff00', theme_variant: 'fruit_salad' }],
  ])
})

test('ignores legacy v1 cache and invalid v2 cache on startup', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  localStorage.setItem('theme-palette-v1', JSON.stringify({ legacy: true }))
  localStorage.setItem(
    'theme-css-vars-v1',
    JSON.stringify(':root{--color-md-primary:#000000;}'),
  )
  localStorage.setItem(PALETTE_KEY, JSON.stringify({ broken: true }))
  localStorage.setItem(CSS_VARS_KEY, JSON.stringify(':root{}'))
  settings.theme_color = undefined

  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())

  await expect
    .poll(() => customStyle())
    .toContain('--color-md-surface-container-lowest:')
})

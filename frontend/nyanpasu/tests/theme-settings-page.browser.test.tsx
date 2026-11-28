import { useSyncExternalStore } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { createTheme } from '@nyanpasu/theme'
import { Route } from '../src/pages/(main)/main/settings/nyanpasu/interface/theme/index'
import { AllPermissions } from './all-permissions'

const backend = vi.hoisted(() => {
  let settings: Record<string, unknown> = {}
  const listeners = new Set<() => void>()

  return {
    get settings() {
      return settings
    },
    reset(next: Record<string, unknown>) {
      settings = next
      listeners.forEach((listener) => listener())
    },
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    save: vi.fn<(patch: Record<string, unknown>) => Promise<void>>(),
  }
})
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({
    value: useSyncExternalStore(backend.subscribe, () => backend.settings)[key],
    upsert: vi.fn(),
  }),
  useSettings: () => ({
    upsert: {
      mutateAsync: async (patch: Record<string, unknown>) => {
        await backend.save(patch)
        backend.reset({ ...backend.settings, ...patch })
      },
    },
  }),
  useSystemAccentColor: () => ({ systemAccentColor: '#1867C0' }),
}))
vi.mock('@/components/page-title', () => ({ PageTitle: () => null }))
const notify = vi.hoisted(() => vi.fn())
vi.mock('@/utils/notification', () => ({ message: notify }))

const SAVED = '#1867C0'

const ThemePage = Route.options.component!

const customStyle = () => document.getElementById('custom-theme')?.innerHTML

// The test renders without CSS, so the empty swatch buttons collapse and
// in-flow labels can cover them; click the element itself rather than its
// on-screen position.
const press = (locator: { element: () => Element }) =>
  (locator.element() as HTMLElement).click()

async function renderPage(
  onTestFinished: (fn: () => void | Promise<void>) => void,
) {
  backend.reset({
    theme_mode: 'light',
    theme_color: SAVED,
    theme_variant: 'tonal_spot',
    theme_contrast_level: 0,
    theme_spec_version: '2025',
  })
  backend.save.mockReset()
  backend.save.mockResolvedValue(undefined)
  notify.mockReset()

  const view = await render(
    <ExperimentalThemeProvider>
      <ThemePage />
    </ExperimentalThemeProvider>,
    { wrapper: AllPermissions },
  )
  onTestFinished(() => view.unmount())

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)

  return {
    view,
    hex: view.getByRole('textbox', {
      name: m.settings_user_interface_theme_color_hex_source(),
    }),
    apply: view.getByRole('button', { name: m.common_apply() }),
    discard: view.getByRole('button', {
      name: m.settings_user_interface_theme_discard(),
    }),
    unsaved: () =>
      view.getByText(m.settings_user_interface_theme_unsaved()).query(),
  }
}

test('editing previews on the mockups and leaves the app theme alone', async ({
  onTestFinished,
}) => {
  const { view, hex, apply } = await renderPage(onTestFinished)

  // The saved color differs from the picker's lowercase hex only in case.
  await expect.element(apply).toBeDisabled()
  expect(
    view
      .getByRole('radio', {
        name: m.settings_user_interface_theme_color_system_accent(),
      })
      .element()
      .getAttribute('aria-checked'),
  ).toBe('true')

  const mockup = () =>
    document.querySelector('[data-slot="theme-mockup"] style')?.textContent

  const before = mockup()
  await hex.fill('#00ff00')

  await expect.poll(mockup).not.toBe(before)
  await expect.element(apply).toBeEnabled()
  expect(customStyle()).toBe(createTheme(SAVED).cssVars)
  expect(backend.save).not.toHaveBeenCalled()
})

test('Apply saves every changed option in one patch', async ({
  onTestFinished,
}) => {
  const { view, apply, unsaved } = await renderPage(onTestFinished)

  press(
    view.getByRole('radio', {
      name: m.settings_user_interface_theme_style_fruit_salad(),
    }),
  )
  press(
    view.getByRole('radio', {
      name: m.settings_user_interface_theme_contrast_high(),
    }),
  )
  press(
    view.getByRole('radio', {
      name: m.settings_user_interface_theme_spec_2021(),
    }),
  )
  await expect.poll(unsaved).not.toBeNull()

  await apply.click()

  await expect
    .poll(() => backend.save.mock.calls)
    .toEqual([
      [
        {
          theme_variant: 'fruit_salad',
          theme_contrast_level: 1,
          theme_spec_version: '2021',
        },
      ],
    ])
  await expect.poll(customStyle).toBe(
    createTheme(SAVED, {
      variant: 'fruitSalad',
      contrastLevel: 1,
      specVersion: '2021',
    }).cssVars,
  )
  await expect.poll(unsaved).toBeNull()
})

test('Discard drops the draft and Restore defaults drafts the defaults', async ({
  onTestFinished,
}) => {
  const { view, hex, apply, discard } = await renderPage(onTestFinished)

  await hex.fill('#00ff00')
  await expect.element(apply).toBeEnabled()

  await discard.click()

  await expect.element(apply).toBeDisabled()
  await expect
    .poll(() => (hex.element() as HTMLInputElement).value)
    .toBe('#1867c0')

  await view
    .getByRole('button', {
      name: m.settings_user_interface_theme_restore_defaults(),
    })
    .click()

  await expect.element(apply).toBeEnabled()
  await expect
    .poll(() => (hex.element() as HTMLInputElement).value)
    .toBe('#3e64a7')
  expect(backend.save).not.toHaveBeenCalled()
})

test('a failed save reports the error and keeps the draft', async ({
  onTestFinished,
}) => {
  const { hex, apply } = await renderPage(onTestFinished)
  backend.save.mockRejectedValue(new Error('boom'))

  await hex.fill('#00ff00')
  await apply.click()

  await expect.poll(() => notify.mock.calls.length).toBe(1)
  await expect.element(apply).toBeEnabled()
  expect((hex.element() as HTMLInputElement).value).toBe('#00ff00')
  expect(customStyle()).toBe(createTheme(SAVED).cssVars)
})

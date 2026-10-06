import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page as browserPage, userEvent } from 'vitest/browser'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { createTheme } from '@nyanpasu/theme'
import ThemeColorConfig from '../src/pages/(main)/main/settings/user-interface/_modules/theme-color-config'

const backend = vi.hoisted(() => ({
  settings: {} as Record<string, string>,
  save: vi.fn<(key: string, value: string) => Promise<void>>(),
  // When false, a saved value reaches the hook only on commitSaved(), like
  // the refetch that lands after the mutation has resolved.
  autoCommit: true,
  commitSaved: () => {},
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => {
    const [value, setValue] = useState(backend.settings[key])
    return {
      value,
      upsert: async (next: string) => {
        await backend.save(key, next)
        if (backend.autoCommit) {
          setValue(next)
        } else {
          backend.commitSaved = () => setValue(next)
        }
      },
    }
  },
  useSystemAccentColor: () => ({ systemAccentColor: '#1867C0' }),
}))
const notify = vi.hoisted(() => vi.fn())
vi.mock('@/utils/notification', () => ({ message: notify }))

const SAVED = '#1867C0'

// Browser tests load no Tailwind; these are the utilities the positioning
// depends on.
const POSITION_STYLES = `
  .relative { position: relative }
  .absolute { position: absolute }
  .right-0 { right: 0 }
  .bottom-0 { bottom: 0 }
  .size-0 { width: 0; height: 0 }
  .w-80 { width: 20rem }
`

const customStyle = () => document.getElementById('custom-theme')?.innerHTML

const page = { leave: () => {} }

function Page() {
  const [shown, setShown] = useState(true)
  page.leave = () => setShown(false)

  return (
    <ExperimentalThemeProvider>
      {shown && <ThemeColorConfig />}
    </ExperimentalThemeProvider>
  )
}

async function openPanel(
  onTestFinished: (fn: () => void | Promise<void>) => void,
) {
  backend.settings = { theme_color: SAVED, theme_mode: 'light' }
  backend.autoCommit = true
  backend.save.mockReset()
  backend.save.mockResolvedValue(undefined)
  notify.mockReset()

  const style = document.createElement('style')
  style.textContent = POSITION_STYLES
  document.head.append(style)
  onTestFinished(() => style.remove())

  const view = await render(<Page />)
  onTestFinished(() => view.unmount())

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)

  await view
    .getByRole('button', {
      name: new RegExp(m.settings_user_interface_theme_color_label()),
    })
    .click()

  await expect
    .element(
      view.getByRole('dialog', {
        name: m.settings_user_interface_theme_color_label(),
      }),
    )
    .toBeVisible()

  const hex = view.getByRole('textbox', {
    name: m.settings_user_interface_theme_color_hex_source(),
  })
  await expect.element(hex).toBeVisible()

  return {
    view,
    hex,
    hexInput: () => hex.element() as HTMLInputElement,
    apply: view.getByRole('button', { name: m.common_apply() }),
  }
}

test('dragging, typing and presets preview the theme without saving it', async ({
  onTestFinished,
}) => {
  const { view, hex, hexInput } = await openPanel(onTestFinished)

  // Home then three PageUp (ten steps each) puts the hue at 30.
  view
    .getByRole('slider', { name: m.settings_user_interface_theme_color_hue() })
    .element()
    .focus()
  await userEvent.keyboard('{Home}{PageUp}{PageUp}{PageUp}')
  await expect.poll(() => hexInput().value).not.toBe('#1867c0')
  await expect.poll(customStyle).toBe(createTheme(hexInput().value).cssVars)

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)

  await view.getByRole('radio', { name: '#3d009e' }).click()
  await expect.poll(customStyle).toBe(createTheme('#3d009e').cssVars)

  expect(backend.save).not.toHaveBeenCalled()
})

test('switching the color mode previews it and never saves it', async ({
  onTestFinished,
}) => {
  const { view } = await openPanel(onTestFinished)
  const root = document.documentElement
  const dark = view.getByRole('radio', {
    name: m.settings_user_interface_theme_mode_dark(),
  })

  await dark.click()
  await expect.poll(() => root.classList.contains('dark')).toBe(true)

  // Clicking the selected mode again keeps it selected.
  await dark.click()
  await expect
    .poll(() => dark.element().getAttribute('aria-checked'))
    .toBe('true')
  expect(root.classList.contains('dark')).toBe(true)

  await userEvent.keyboard('{Escape}')

  await expect.poll(() => root.classList.contains('light')).toBe(true)
  expect(root.classList.contains('dark')).toBe(false)
  expect(backend.save).not.toHaveBeenCalled()
})

test('Apply saves the color once and closes after the saved value arrives', async ({
  onTestFinished,
}) => {
  const { hex, apply } = await openPanel(onTestFinished)
  backend.autoCommit = false

  await hex.fill('#00ff00')
  await apply.click()

  await expect
    .poll(() => backend.save.mock.calls)
    .toEqual([['theme_color', '#00ff00']])
  // The mutation resolved but the refetch has not landed: closing now
  // would flash the old color.
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(hex.query()).not.toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)

  backend.commitSaved()

  await expect.poll(() => hex.query()).toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)
})

test('closing without applying restores the saved theme', async ({
  onTestFinished,
}) => {
  const { view, hex, apply } = await openPanel(onTestFinished)

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

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)
  await expect.element(apply).toBeEnabled()

  await userEvent.keyboard('{Escape}')

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)
  expect(backend.save).not.toHaveBeenCalled()
})

test('leaving the page with the panel open restores the saved theme', async ({
  onTestFinished,
}) => {
  const { hex } = await openPanel(onTestFinished)

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)

  page.leave()

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)
  expect(backend.save).not.toHaveBeenCalled()
})

test('a failed save keeps the panel open and reports the error', async ({
  onTestFinished,
}) => {
  const { hex, apply } = await openPanel(onTestFinished)
  backend.save.mockRejectedValue(new Error('boom'))

  await hex.fill('#00ff00')
  await apply.click()

  await expect.poll(() => notify.mock.calls.length).toBe(1)
  expect(hex.query()).not.toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)
  await expect.element(apply).toBeEnabled()
})

test('the panel is anchored to the card corner and slides up inside the window', async ({
  onTestFinished,
}) => {
  await browserPage.viewport(800, 636)

  await openPanel(onTestFinished)

  const dialog = () =>
    document.querySelector<HTMLElement>('[data-slot="popover-content"]')!
  const card = () =>
    document.querySelector<HTMLElement>(
      '[data-slot="theme-color-config-card"]',
    )!

  await expect.poll(() => dialog().dataset.side).toBe('left')
  await expect
    .poll(() =>
      Math.abs(
        dialog().getBoundingClientRect().right -
          card().getBoundingClientRect().right,
      ),
    )
    .toBeLessThanOrEqual(1)
  await expect
    .poll(() => dialog().getBoundingClientRect().bottom)
    .toBeLessThanOrEqual(window.innerHeight - 12)
})

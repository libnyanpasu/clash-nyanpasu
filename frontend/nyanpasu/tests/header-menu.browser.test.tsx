import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { m } from '@/paraglide/messages'
import HeaderMenu from '../src/pages/(main)/_modules/header-menu'

const fakes = vi.hoisted(() => ({
  setThemeMode: vi.fn(),
  executeSystemProxy: vi.fn(),
  openWiki: vi.fn(),
  openIssueReport: vi.fn(),
  collectLogs: vi.fn(),
}))

vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({ hasNewVersion: true, isReady: false }),
}))
vi.mock('@/components/providers/language-provider', () => ({
  useLanguage: () => ({ language: 'en', setLanguage: vi.fn() }),
}))
vi.mock('@/components/providers/theme-provider', () => ({
  useExperimentalThemeContext: () => ({
    themeMode: 'light',
    setThemeMode: fakes.setThemeMode,
  }),
}))
vi.mock('@/hooks/use-proxy-settings', () => ({
  useSystemProxy: () => ({
    isActive: false,
    isPending: false,
    execute: fakes.executeSystemProxy,
  }),
  useTunMode: () => ({ isActive: false, isPending: false, execute: vi.fn() }),
}))
vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useProxyMode: () => ({
    value: { global: false, direct: false, rule: true, script: false },
    upsert: vi.fn(),
  }),
}))
vi.mock('@/services/rpc', () => ({ rpc: { collectLogs: fakes.collectLogs } }))
vi.mock('../src/pages/(main)/_modules/help-links', () => ({
  openWiki: fakes.openWiki,
  openIssueReport: fakes.openIssueReport,
}))
vi.mock('@nyanpasu/platform', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/platform')>()),
  isBrowser: () => false,
}))
vi.mock('../src/pages/(main)/main/profiles/$type/index', () => ({
  Action: { ImportLocalProfile: 'import-local-profile' },
}))
vi.mock('@tanstack/react-router', async (importOriginal) => {
  const original =
    await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...original,
    Link: ({
      to,
      children,
      ...props
    }: React.ComponentProps<'a'> & { to: string }) => (
      <a href={to} {...props}>
        {children}
      </a>
    ),
  }
})

test('top-level arrows switch menus and Escape returns focus to the active trigger', async () => {
  await render(<HeaderMenu />)

  const file = page.getByRole('menuitem', {
    name: m.header_file_action_title(),
  })
  const settings = page.getByRole('menuitem', {
    name: m.header_settings_action_title(),
  })

  expect(file.element().getBoundingClientRect().height).toBe(32)

  await file.click()
  await userEvent.keyboard('{ArrowRight}')
  await expect.element(settings).toHaveAttribute('aria-expanded', 'true')
  await userEvent.keyboard('{Escape}')
  await expect.element(settings).toHaveFocus()
})

test('settings menu options call the theme and system proxy handlers', async () => {
  await render(<HeaderMenu />)

  await page
    .getByRole('menuitem', { name: m.header_settings_action_title() })
    .click()
  await page
    .getByRole('menuitem', { name: m.header_settings_action_theme_mode() })
    .click()
  await page
    .getByRole('menuitemradio', {
      name: m.settings_user_interface_theme_mode_dark(),
    })
    .click()
  expect(fakes.setThemeMode).toHaveBeenCalledWith('dark')

  await page
    .getByRole('menuitem', { name: m.header_settings_action_title() })
    .click()
  await page
    .getByRole('menuitem', {
      name: m.header_settings_action_proxy_settings(),
    })
    .click()
  await page
    .getByRole('menuitemcheckbox', {
      name: m.settings_system_proxy_system_proxy_label(),
    })
    .click()
  expect(fakes.executeSystemProxy).toHaveBeenCalledOnce()
})

test('the update link stays outside the menubar and Help invokes its action', async () => {
  const view = await render(<HeaderMenu />)

  const update = page.getByRole('link', { name: m.header_new_version() })
  await expect
    .element(update)
    .toHaveAttribute('href', '/main/settings/nyanpasu/updates')
  expect(
    view.container
      .querySelector('[data-slot="menubar"]')
      ?.contains(update.element()),
  ).toBe(false)

  await page
    .getByRole('menuitem', { name: m.header_help_action_title() })
    .click()
  await page
    .getByRole('menuitem', { name: m.header_help_action_wiki() })
    .click()
  expect(fakes.openWiki).toHaveBeenCalledOnce()
})

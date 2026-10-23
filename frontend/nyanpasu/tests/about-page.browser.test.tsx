import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import { Route } from '../src/pages/(main)/main/settings/about/index'

const rpc = vi.hoisted(() => ({
  openThat: vi.fn(),
  reportIssue: vi.fn(),
}))

vi.mock('@/services/rpc', () => ({ rpc }))
vi.mock('@nyanpasu/platform', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/platform')>()),
  isTauri: () => true,
}))
vi.mock('@/components/page-title', () => ({
  PageTitle: ({ children }: { children: string }) => <h1>{children}</h1>,
}))
vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({ currentVersion: '2.0.0' }),
}))
vi.mock('@/components/logo/animated-logo', () => ({
  default: ({ className }: { className?: string }) => (
    <div className={className} data-slot="animated-logo" />
  ),
}))

const AboutPage = Route.options.component ?? (() => null)

test('About shows the version and opens the help links', async () => {
  const view = await render(<AboutPage />)

  await expect
    .element(
      view.getByText(m.settings_label_about_version({ version: '2.0.0' })),
    )
    .toBeInTheDocument()
  const logo = view.container.querySelector<HTMLElement>(
    '[data-slot="animated-logo"]',
  )!
  expect(logo.getBoundingClientRect().width).toBe(128)
  expect(logo.getBoundingClientRect().height).toBe(128)

  await view
    .getByRole('button', { name: m.header_help_action_wiki(), exact: false })
    .click()
  await expect
    .poll(() => rpc.openThat)
    .toHaveBeenCalledWith('https://nyanpasu.org')

  await view.getByRole('button', { name: 'GitHub', exact: false }).click()
  await expect
    .poll(() => rpc.openThat)
    .toHaveBeenCalledWith('https://github.com/libnyanpasu/clash-nyanpasu')

  rpc.reportIssue.mockResolvedValue({ status: 'ok', data: null })
  await view
    .getByRole('button', { name: m.header_help_action_issues(), exact: false })
    .click()
  await expect
    .poll(() => rpc.reportIssue)
    .toHaveBeenCalledWith(
      expect.objectContaining({ userAgent: expect.any(String) }),
    )
})

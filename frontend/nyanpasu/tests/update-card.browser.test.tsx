import { beforeEach, expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { Card } from '@nyanpasu/ui/card'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import type { AppUpdateChangelog, AppUpdateSnapshot } from '@nyanpasu/rpc/types'
import UpdateControls from '../src/pages/(main)/main/settings/nyanpasu/updates/_modules/update-controls'

function UpdateCard() {
  return (
    <TooltipProvider>
      <Card>
        <UpdateControls />
      </Card>
    </TooltipProvider>
  )
}

const state = vi.hoisted(() => ({
  snapshot: {
    revision: 1,
    phase: 'available',
    release: { version: '2.1.0', date: null, body: 'Release notes' },
    downloaded: 0,
    total: null,
    speed: 0,
    source: null,
    error: null,
    last_checked_at: null,
    supported: true,
    endpoints: ['https://updates.example.test/stable'],
  } as AppUpdateSnapshot,
  check: vi.fn(),
  download: vi.fn(),
  cancelDownload: vi.fn(),
  install: vi.fn(),
  discardPackage: vi.fn(),
  pending: false,
  isDesktop: true,
  changelog: undefined as AppUpdateChangelog | undefined,
  changelogRange: vi.fn(),
  openThat: vi.fn(),
}))

const initialSnapshot = state.snapshot

beforeEach(() => {
  state.snapshot = { ...initialSnapshot }
  state.pending = false
  state.isDesktop = true
  state.changelog = undefined
  vi.clearAllMocks()
})

vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({
    currentVersion: '2.0.0',
    snapshot: state.snapshot,
    isDesktop: state.isDesktop,
    isLoading: false,
    isPending: state.pending,
    check: state.check,
    download: state.download,
    cancelDownload: state.cancelDownload,
    install: state.install,
    discardPackage: state.discardPackage,
  }),
}))
vi.mock('@nyanpasu/query', () => ({
  useAppUpdateChangelog: (from: string, to: string) => {
    state.changelogRange(from, to)
    return { data: state.changelog, error: null, isPending: false }
  },
}))
vi.mock('@/services/rpc', () => ({ commands: { openThat: state.openThat } }))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

test('check button stays visible with loading while backend checks for updates', async ({
  onTestFinished,
}) => {
  const previous = state.snapshot
  state.snapshot = { ...state.snapshot, phase: 'idle', release: null }
  state.check.mockReset()
  const view = await render(<UpdateCard />)
  onTestFinished(async () => {
    await view.unmount()
    state.snapshot = previous
  })

  const idleButton = view.getByRole('button', {
    name: m.settings_about_update_idle(),
  })
  await idleButton.click()
  expect(state.check).toHaveBeenCalledOnce()
  state.snapshot = { ...state.snapshot, phase: 'checking' }
  await view.rerender(<UpdateCard />)

  const checkingButton = view.getByRole('button', {
    name: m.settings_about_update_idle(),
  })
  await expect.element(checkingButton).toBeVisible()
  await expect.element(checkingButton).toBeDisabled()
  expect(
    view.container.querySelector('[data-slot="button-loading"]'),
  ).toBeNull()
  expect(
    view.container.querySelector('[data-slot="circular-progress"]'),
  ).not.toBeNull()
  const status = view.container.querySelector<HTMLElement>(
    '[data-slot="about-update-actions"] [data-slot="about-update-label"]',
  )
  const statusButton = status?.closest('button')
  expect(status?.classList.contains('whitespace-nowrap')).toBe(true)
  expect(
    statusButton?.querySelector('[data-slot="update-button-action"]'),
  ).toBeNull()
  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(m.settings_about_update_checking())
})

test('checking completion keeps the action label mounted', async ({
  onTestFinished,
}) => {
  const previous = state.snapshot
  state.snapshot = {
    ...state.snapshot,
    phase: 'checking',
    release: null,
    last_checked_at: '2026-10-03T12:00:00Z',
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => {
    await view.unmount()
    state.snapshot = previous
  })

  const status = view.container.querySelector(
    '[data-slot="about-update-actions"] [data-slot="about-update-label"]',
  )
  expect(status).not.toBeNull()

  state.snapshot = { ...state.snapshot, phase: 'up_to_date' }
  await view.rerender(<UpdateCard />)

  expect(
    view.container.querySelector(
      '[data-slot="about-update-actions"] [data-slot="about-update-label"]',
    ),
  ).toBe(status)
  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(m.settings_label_about_update_no_update())
  const check = view.getByRole('button', {
    name: m.settings_about_update_idle(),
  })
  await expect.element(check).toBeEnabled()
  await check.click()
  expect(state.check).toHaveBeenCalledOnce()
})

test('rapid phase changes show only the current status and action text', async ({
  onTestFinished,
}) => {
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())
  const action = view.container.querySelector(
    '[data-slot="about-update-actions"] button',
  )!

  for (const [phase, status, label] of [
    [
      'downloading',
      m.settings_about_update_downloading(),
      m.settings_about_update_cancel_download(),
    ],
    [
      'verifying',
      m.settings_about_update_verifying(),
      m.settings_about_update_install(),
    ],
    [
      'installing',
      m.settings_label_about_update_installing(),
      m.settings_about_update_install(),
    ],
    [
      'ready',
      m.settings_about_update_ready(),
      m.settings_about_update_install(),
    ],
  ] as const) {
    state.snapshot = { ...state.snapshot, phase }
    await view.rerender(<UpdateCard />)
    expect(
      view.container.querySelector('[data-slot="about-update-actions"] button'),
    ).toBe(action)
    expect(action.textContent).toBe(label)
    expect(view.container.querySelector('[role="status"]')?.textContent).toBe(
      status,
    )
  }
})

test('verifying never offers installation before signature verification finishes', async ({
  onTestFinished,
}) => {
  state.snapshot = {
    ...state.snapshot,
    phase: 'verifying',
    downloaded: 1024,
    total: null,
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await expect
    .element(view.getByRole('progressbar'))
    .not.toHaveAttribute('aria-valuenow')
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_about_update_install(),
      }),
    )
    .toBeDisabled()
  expect(state.install).not.toHaveBeenCalled()
})

test('unknown download size uses indeterminate progress and keeps cancellation available', async ({
  onTestFinished,
}) => {
  state.snapshot = {
    ...state.snapshot,
    phase: 'downloading',
    downloaded: 1024,
    total: null,
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  const progress = view.getByRole('progressbar')
  await expect.element(progress).not.toHaveAttribute('aria-valuenow')
  const cancel = view.getByRole('button', {
    name: m.settings_about_update_cancel_download(),
  })
  await expect.element(cancel).toBeEnabled()
  await cancel.click()
  expect(state.cancelDownload).toHaveBeenCalledOnce()
})

test('ready package offers installation only after an explicit click', async ({
  onTestFinished,
}) => {
  state.snapshot = {
    ...state.snapshot,
    phase: 'ready',
    downloaded: 1024,
    total: 1024,
    source: 'github',
  }
  state.install.mockReset()
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  const install = view.getByRole('button', {
    name: m.settings_about_update_install(),
  })
  await expect.element(install).toBeEnabled()
  expect(state.install).not.toHaveBeenCalled()
  await install.click()
  expect(state.install).toHaveBeenCalledOnce()
})

test('available updates have one primary action and keep rechecking in the menu', async ({
  onTestFinished,
}) => {
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()
  const actions = view.container.querySelector(
    '[data-slot="about-update-actions"]',
  )!
  expect(actions.querySelectorAll('button')).toHaveLength(1)
  await expect
    .element(
      view.getByRole('button', { name: m.settings_about_update_check_again() }),
    )
    .not.toBeInTheDocument()
  await view
    .getByRole('button', {
      name: m.settings_label_about_update_to_update_button(),
    })
    .click()
  expect(state.download).toHaveBeenCalledOnce()
  expect(state.install).not.toHaveBeenCalled()

  await view
    .getByRole('button', { name: m.settings_about_update_more_actions() })
    .click()
  await view
    .getByRole('menuitem', { name: m.settings_about_update_check_again() })
    .click()
  expect(state.check).toHaveBeenCalledOnce()
})

test('ready packages can be discarded from the secondary menu', async ({
  onTestFinished,
}) => {
  state.snapshot = { ...state.snapshot, phase: 'ready' }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await expect
    .element(
      view.getByRole('button', { name: m.settings_about_update_discard() }),
    )
    .not.toBeInTheDocument()
  await view
    .getByRole('button', { name: m.settings_about_update_more_actions() })
    .click()
  await view
    .getByRole('menuitem', { name: m.settings_about_update_discard() })
    .click()
  expect(state.discardPackage).toHaveBeenCalledOnce()
  expect(state.install).not.toHaveBeenCalled()
})

test('pending operations disable both the primary action and the secondary menu', async ({
  onTestFinished,
}) => {
  state.pending = true
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_label_about_update_to_update_button(),
      }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_about_update_more_actions(),
      }),
    )
    .toBeDisabled()
  expect(state.download).not.toHaveBeenCalled()
})

test.each([
  { environment: 'browser', desktop: false, supported: true },
  { environment: 'unsupported installation', desktop: true, supported: false },
])(
  '$environment offers GitHub releases without native update actions',
  async ({ desktop, supported }) => {
    state.isDesktop = desktop
    state.snapshot = { ...state.snapshot, supported }
    const view = await render(<UpdateCard />)
    onTestFinished(async () => view.unmount())

    await expect
      .element(
        view.getByRole('button', {
          name: m.settings_label_about_update_to_github_releases(),
        }),
      )
      .toBeEnabled()
    expect(view.container.querySelectorAll('button')).toHaveLength(1)
    expect(state.check).not.toHaveBeenCalled()
    expect(state.download).not.toHaveBeenCalled()
  },
)

test.each(['cancelled', 'failed'] as const)(
  '%s downloads offer an explicit download action',
  async (phase) => {
    state.snapshot = { ...state.snapshot, phase }
    const view = await render(<UpdateCard />)
    onTestFinished(async () => view.unmount())

    const label =
      phase === 'failed'
        ? m.settings_about_update_retry_download()
        : m.settings_label_about_update_to_update_button()
    await view.getByRole('button', { name: label }).click()
    expect(state.download).toHaveBeenCalledOnce()
    expect(state.check).not.toHaveBeenCalled()
  },
)

test('failed checks without a release retry checking rather than downloading', async ({
  onTestFinished,
}) => {
  state.snapshot = {
    ...state.snapshot,
    phase: 'failed',
    release: null,
    error: 'Network unavailable',
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await expect
    .element(view.getByRole('alert'))
    .toHaveTextContent('Network unavailable')
  await view
    .getByRole('button', { name: m.settings_about_update_idle() })
    .click()
  expect(state.check).toHaveBeenCalledOnce()
  expect(state.download).not.toHaveBeenCalled()
})

test.each(['cancelling', 'installing'] as const)(
  '%s cannot start another operation',
  async (phase) => {
    state.snapshot = { ...state.snapshot, phase }
    const view = await render(<UpdateCard />)
    onTestFinished(async () => view.unmount())

    const label =
      phase === 'cancelling'
        ? m.settings_about_update_cancel_download()
        : m.settings_about_update_install()
    await expect
      .element(view.getByRole('button', { name: label }))
      .toBeDisabled()
    await expect
      .element(
        view.getByRole('button', {
          name: m.settings_about_update_more_actions(),
        }),
      )
      .not.toBeInTheDocument()
  },
)

test('the changelog dialog can close while a background download continues', async ({
  onTestFinished,
}) => {
  state.snapshot = {
    ...state.snapshot,
    phase: 'downloading',
    downloaded: 1024,
    total: 4096,
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await view
    .getByRole('button', { name: m.settings_about_update_changelog() })
    .click()
  await expect.element(view.getByRole('dialog')).toBeVisible()
  await view.getByRole('button', { name: m.common_close() }).click()
  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_about_update_cancel_download(),
      }),
    )
    .toBeEnabled()
})

test('the changelog lists every version since the installed one', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', true)
  onTestFinished(() => {
    vi.unstubAllGlobals()
  })
  state.changelog = {
    entries: [
      {
        version: '2.1.0',
        date: '2026-11-02',
        body: '- Stable change by @keiko233',
        source: 'changelog',
      },
      {
        version: '2.1.0-rc.1',
        date: '2026-11-01',
        body: '- Prerelease change',
        source: 'release',
      },
    ],
    compare_url:
      'https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0...v2.1.0',
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await view
    .getByRole('button', { name: m.settings_about_update_changelog() })
    .click()
  expect(state.changelogRange).toHaveBeenLastCalledWith('2.0.0', '2.1.0')
  const dialog = view.getByRole('dialog')
  await expect
    .element(dialog.getByRole('heading', { name: 'v2.1.0', exact: true }))
    .toBeVisible()
  await expect
    .element(dialog.getByRole('heading', { name: 'v2.1.0-rc.1' }))
    .toBeVisible()
  await expect
    .element(dialog.getByText('Release notes'))
    .not.toBeInTheDocument()

  await dialog.getByRole('link', { name: '@keiko233' }).click()
  expect(state.openThat).toHaveBeenLastCalledWith('https://github.com/keiko233')
  await dialog
    .getByRole('button', { name: m.settings_about_update_changelog_compare() })
    .click()
  expect(state.openThat).toHaveBeenLastCalledWith(
    'https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0...v2.1.0',
  )
})

test('the release body stands in when no version notes resolve', async ({
  onTestFinished,
}) => {
  state.changelog = { entries: [], compare_url: null }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => view.unmount())

  await view
    .getByRole('button', { name: m.settings_about_update_changelog() })
    .click()
  await expect
    .element(view.getByRole('dialog').getByText('Release notes'))
    .toBeVisible()
})

test('last checked timestamp is shown in a tooltip', async ({
  onTestFinished,
}) => {
  const previous = state.snapshot
  const lastCheckedAt = '2026-10-03T12:00:00Z'
  state.snapshot = {
    ...state.snapshot,
    phase: 'idle',
    last_checked_at: lastCheckedAt,
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => {
    await view.unmount()
    state.snapshot = previous
  })

  const lastChecked = m.settings_about_update_last_checked({
    date: new Date(lastCheckedAt).toLocaleString(),
  })
  expect(view.container.textContent).not.toContain(lastChecked)
  await view
    .getByRole('button', { name: m.settings_about_update_idle() })
    .hover()
  await expect.element(view.getByRole('tooltip')).toHaveTextContent(lastChecked)
})

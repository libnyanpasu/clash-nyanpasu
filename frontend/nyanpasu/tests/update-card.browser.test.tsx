import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { Card } from '@nyanpasu/ui/card'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import type { AppUpdateSnapshot } from '@nyanpasu/rpc/types'
import UpdateControls from '../src/pages/(main)/main/settings/about/_modules/update-controls'

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
}))

vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({
    snapshot: state.snapshot,
    isDesktop: true,
    isLoading: false,
    isPending: state.pending,
    check: state.check,
    download: state.download,
    cancelDownload: state.cancelDownload,
    install: state.install,
    discardPackage: state.discardPackage,
  }),
}))
vi.mock('@/services/rpc', () => ({ commands: { openThat: vi.fn() } }))
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
    name: m.settings_about_update_checking(),
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
    '[data-slot="action-swap-text"]',
  )
  const statusButton = status?.closest('button')
  expect(status?.parentElement?.classList.contains('flex')).toBe(true)
  expect(status?.classList.contains('whitespace-nowrap')).toBe(true)
  expect(
    statusButton?.querySelector('[data-slot="update-button-action"]'),
  ).toBeNull()
  expect(statusButton?.textContent).toContain(
    m.settings_about_update_checking(),
  )
  await expect
    .element(checkingButton)
    .toHaveTextContent(m.settings_about_update_checking())
})

test('up-to-date transition keeps the action swap mounted', async ({
  onTestFinished,
}) => {
  const previous = state.snapshot
  state.snapshot = {
    ...state.snapshot,
    phase: 'checking',
    last_checked_at: '2026-10-03T12:00:00Z',
  }
  const view = await render(<UpdateCard />)
  onTestFinished(async () => {
    await view.unmount()
    state.snapshot = previous
  })

  const status = view.container.querySelector('[data-slot="action-swap-text"]')
  expect(status).not.toBeNull()

  state.snapshot = { ...state.snapshot, phase: 'up_to_date' }
  await view.rerender(<UpdateCard />)

  expect(view.container.querySelector('[data-slot="action-swap-text"]')).toBe(
    status,
  )
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
        name: m.settings_about_update_verifying(),
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
    name: m.settings_about_update_downloading(),
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
    name: m.settings_about_update_ready(),
  })
  await expect.element(install).toBeEnabled()
  expect(state.install).not.toHaveBeenCalled()
  await install.click()
  expect(state.install).toHaveBeenCalledOnce()
})

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
        name: m.settings_about_update_downloading(),
      }),
    )
    .toBeEnabled()
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

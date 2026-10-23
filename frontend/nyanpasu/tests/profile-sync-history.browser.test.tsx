import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { m } from '@/paraglide/messages'
import type { RunDto } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import SyncHistorySection from '../src/pages/(main)/main/profiles/$type/detail/_modules/sync-history-section'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

const triggerSelector = '[data-slot="accordion-trigger"]'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

function mount(
  onTestFinished: (fn: () => void) => void,
  queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  }),
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    clearMocks()
  })
  root.render(
    <QueryClientProvider client={queries}>
      <SyncHistorySection uid="p1" />
    </QueryClientProvider>,
  )
  return container
}

const triggers = (container: HTMLElement) =>
  container.querySelectorAll<HTMLElement>(triggerSelector)

const buttonWithText = (container: HTMLElement, ...labels: string[]) =>
  Array.from(container.querySelectorAll('button')).find((button) =>
    labels.includes(button.textContent ?? ''),
  )

test('live state replaces admission history and each run opens on its own', async ({
  onTestFinished,
}) => {
  const run: RunDto = {
    id: 'active',
    job: 'profiles/sync/p1',
    definition_version: '1',
    admission_sequence: '9007199254740993',
    trigger: 'manual',
    scheduled_at: null,
    admitted_at: '2026-09-30T00:00:00Z',
    finished_at: null,
    state: { kind: 'admitted' },
    last_log_sequence: '0',
    dropped_log_count: '0',
  }
  const failed: RunDto = {
    ...run,
    id: 'failed',
    admission_sequence: '9007199254740992',
    finished_at: run.admitted_at,
    state: {
      kind: 'finished',
      completion: {
        outcome: { kind: 'failed', code: 'http', message: 'HTTP 403' },
        output: { kind: 'none' },
        journal: { kind: 'durable' },
      },
    },
  }
  const requested: string[] = []
  mockIPC((wireCommand, args) => {
    expect(wireCommand).toBe('call_rpc')
    const { method: command, params: input } = args as {
      method: string
      params: { uid: string; run: string }
    }
    expect(input.uid).toBe('p1')
    switch (command) {
      case 'get_profile_sync_status':
        return {
          scheduled: true,
          next_run_at: '2026-09-30T01:00:00Z',
          active: [{ ...run, state: { kind: 'running' } }],
          journal_degraded: false,
          registration_error: null,
          history_limit: 10,
        }
      case 'get_profile_sync_runs':
        return { items: [run, failed], next: null }
      case 'get_profile_sync_logs':
        requested.push(input.run)
        return {
          items: [
            {
              run_id: input.run,
              sequence: '1',
              time: run.admitted_at,
              level: 'INFO',
              target: 'nyanpasu::profile_sync',
              fields: { message: `${input.run} log` },
            },
          ],
          next: null,
        }
      default:
        throw new Error(`Unexpected command: ${command}`)
    }
  })
  const container = mount(onTestFinished)

  await expect.poll(() => triggers(container).length).toBe(2)
  expect(triggers(container)[0].textContent).toContain(m.profile_sync_running())
  expect(container.textContent).toContain(
    m.profile_sync_retention({ count: 10 }),
  )
  // Every run starts collapsed, and nothing is fetched for it yet.
  expect(triggers(container)[0].getAttribute('aria-expanded')).toBe('false')
  expect(triggers(container)[1].getAttribute('aria-expanded')).toBe('false')
  expect(requested).toEqual([])

  triggers(container)[0].click()
  await expect.poll(() => container.textContent).toContain('active log')
  expect(triggers(container)[0].getAttribute('aria-expanded')).toBe('true')

  triggers(container)[1].click()
  await expect.poll(() => container.textContent).toContain('failed log')
  expect(container.textContent).toContain('HTTP 403')
  expect(triggers(container)[0].getAttribute('aria-expanded')).toBe('false')
  expect(triggers(container)[1].getAttribute('aria-expanded')).toBe('true')
  expect(requested).toEqual(['active', 'failed'])

  // The open run can be collapsed again.
  triggers(container)[1].click()
  await expect
    .poll(() => triggers(container)[1].getAttribute('aria-expanded'))
    .toBe('false')
})

test('older runs unfold inline below the latest runs and fold away again', async ({
  onTestFinished,
}) => {
  const runAt = (sequence: number): RunDto => ({
    id: `run-${sequence}`,
    job: 'profiles/sync/p1',
    definition_version: '1',
    admission_sequence: String(sequence),
    trigger: 'scheduled',
    scheduled_at: null,
    admitted_at: '2026-09-30T00:00:00Z',
    finished_at: '2026-09-30T00:00:01Z',
    state: {
      kind: 'finished',
      completion: {
        outcome: { kind: 'succeeded' },
        output: { kind: 'none' },
        journal: { kind: 'durable' },
      },
    },
    last_log_sequence: '1',
    dropped_log_count: '2',
  })
  const requested: string[] = []
  mockIPC((_wireCommand, args) => {
    const { method: command, params: input } = args as {
      method: string
      params: { run: string; after: unknown }
    }
    switch (command) {
      case 'get_profile_sync_status':
        return {
          scheduled: false,
          next_run_at: null,
          active: [],
          journal_degraded: false,
          registration_error: null,
          history_limit: 10,
        }
      case 'get_profile_sync_runs':
        return input.after
          ? { items: [runAt(1)], next: null }
          : {
              items: [runAt(5), runAt(4), runAt(3), runAt(2)],
              next: { sequence: '2', id: 'run-2' },
            }
      case 'get_profile_sync_logs':
        requested.push(input.run)
        return {
          items: [
            {
              run_id: input.run,
              sequence: '1',
              time: '2026-09-30T00:00:00Z',
              level: 'INFO',
              target: 'nyanpasu::profile_sync',
              fields: { message: `${input.run} log` },
            },
          ],
          next: null,
        }
      default:
        throw new Error(`Unexpected command: ${command}`)
    }
  })
  const container = mount(onTestFinished)
  const toggle = () =>
    buttonWithText(
      container,
      m.profile_detail_history_view_all(),
      m.profile_detail_history_hide(),
    )!
  const olderButton = () => buttonWithText(container, m.profile_sync_older())

  await expect.poll(() => triggers(container).length).toBe(3)
  expect(toggle().textContent).toBe(m.profile_detail_history_view_all())
  expect(toggle().getAttribute('aria-expanded')).toBe('false')
  // "Load older" only appears once the list is expanded.
  expect(olderButton()).toBeUndefined()

  // The latest runs open in place before anything is expanded.
  triggers(container)[0].click()
  await expect.poll(() => container.textContent).toContain('run-5 log')

  // Expanding adds the remaining runs to the same list, not to a drawer.
  toggle().click()
  await expect.poll(() => triggers(container).length).toBe(4)
  expect(toggle().textContent).toBe(m.profile_detail_history_hide())
  expect(toggle().getAttribute('aria-expanded')).toBe('true')
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(container.querySelectorAll('[data-slot="accordion"]')).toHaveLength(1)

  // One run is open at a time across the latest and the older runs.
  triggers(container)[3].click()
  await expect.poll(() => container.textContent).toContain('run-2 log')
  expect(triggers(container)[0].getAttribute('aria-expanded')).toBe('false')
  expect(triggers(container)[3].getAttribute('aria-expanded')).toBe('true')

  // Older pages load while expanded.
  olderButton()!.click()
  await expect.poll(() => triggers(container).length).toBe(5)
  expect(olderButton()).toBeUndefined()

  // Folding the older runs away closes the one that was open among them.
  toggle().click()
  await expect.poll(() => triggers(container).length).toBe(3)
  expect(toggle().textContent).toBe(m.profile_detail_history_view_all())
  expect(
    Array.from(triggers(container)).every(
      (trigger) => trigger.getAttribute('aria-expanded') === 'false',
    ),
  ).toBe(true)
  expect(requested).toEqual(['run-5', 'run-2'])
})

test('an idle profile stops polling and a finished run refreshes once', async ({
  onTestFinished,
}) => {
  const finished: RunDto = {
    id: 'done',
    job: 'profiles/sync/p1',
    definition_version: '1',
    admission_sequence: '1',
    trigger: 'manual',
    scheduled_at: null,
    admitted_at: '2026-09-30T00:00:00Z',
    finished_at: '2026-09-30T00:00:01Z',
    state: {
      kind: 'finished',
      completion: {
        outcome: { kind: 'succeeded' },
        output: { kind: 'none' },
        journal: { kind: 'durable' },
      },
    },
    last_log_sequence: '1',
    dropped_log_count: '0',
  } as RunDto
  const running: RunDto = {
    ...finished,
    id: 'live',
    admission_sequence: '2',
    finished_at: null,
    state: { kind: 'running' },
  } as RunDto
  let active = true
  const calls = { status: 0, runs: 0, logs: 0 }
  mockIPC((_wireCommand, args) => {
    const { method: command } = args as { method: string }
    switch (command) {
      case 'get_profile_sync_status':
        calls.status += 1
        return {
          scheduled: true,
          next_run_at: '2999-01-01T00:00:00Z',
          active: active ? [running] : [],
          journal_degraded: false,
          registration_error: null,
          history_limit: 10,
        }
      case 'get_profile_sync_runs':
        calls.runs += 1
        return {
          items: active
            ? [finished]
            : [{ ...running, state: finished.state }, finished],
          next: null,
        }
      case 'get_profile_sync_logs':
        calls.logs += 1
        return { items: [], next: null }
      default:
        throw new Error(`Unexpected command: ${command}`)
    }
  })
  const container = mount(onTestFinished)

  // Logs are only watched for the run that is open.
  await expect.poll(() => triggers(container).length).toBe(2)
  triggers(container)[0].click()

  // A live run keeps the status and its logs polling.
  await expect.poll(() => calls.logs, { timeout: 5000 }).toBeGreaterThan(1)
  expect(calls.status).toBeGreaterThan(1)

  active = false
  // The run leaves the active set: the history and the logs refresh once.
  await expect.poll(() => calls.runs, { timeout: 5000 }).toBe(2)
  await new Promise((resolve) => setTimeout(resolve, 500))
  const settled = { ...calls }
  await new Promise((resolve) => setTimeout(resolve, 4500))
  expect(calls).toEqual(settled)
})

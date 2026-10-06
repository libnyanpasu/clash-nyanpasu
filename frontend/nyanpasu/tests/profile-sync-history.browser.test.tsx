import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { m } from '@/paraglide/messages'
import type { RunDto } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { SyncHistoryCard } from '../src/pages/(main)/main/profiles/$type/detail/_modules/sync-history-card'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

test('live state replaces admission history and selecting a run loads its logs', async ({
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
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
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
      <SyncHistoryCard uid="p1" />
    </QueryClientProvider>,
  )
  await expect.poll(() => container.textContent).toContain('active log')
  const buttons = container.querySelectorAll('button')
  expect(buttons).toHaveLength(2)
  expect(buttons[0].textContent).toContain(m.profile_sync_running())
  expect(buttons[0].getAttribute('aria-pressed')).toBe('true')
  expect(container.textContent).toContain(
    m.profile_sync_retention({ count: 10 }),
  )
  buttons[1].click()
  await expect.poll(() => container.textContent).toContain('failed log')
  expect(container.textContent).toContain('HTTP 403')
  expect(requested).toEqual(['active', 'failed'])
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
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
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
      <SyncHistoryCard uid="p1" />
    </QueryClientProvider>,
  )
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

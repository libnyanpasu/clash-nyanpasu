import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { Channel } from '@tauri-apps/api/core'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import type { TrafficSummary, TrafficSummaryFrame } from '../src/ipc/bindings'
import {
  ClashWSProvider,
  useClashWSHistory,
  useClashWSStatus,
} from '../src/provider/clash-ws-provider'

const bytes = { upload: '0', download: '0' }
function summary(id: string, revision: string, fresh = true): TrafficSummary {
  return {
    session: {
      id,
      host: 'host',
      instance_id: id,
      process_started_at: null,
      attached_at: '1',
      first_sample_at: '1',
      last_sample_at: '1',
      ended_at: null,
      core_reported_bytes: bytes,
      attributed_bytes: bytes,
      time_unallocated: bytes,
      global_counters: null,
      last_monotonic_ns: null,
      source_generation: '1',
      position: { sequence: revision, digest: 'digest' },
      quality: [],
      freshness: fresh ? 'Fresh' : 'Stale',
      observed_connections: '1',
    },
    revision,
    current_rate: fresh ? { upload: 9, download: 7 } : null,
    active_connections: '1',
    member_rates: { proxy: fresh ? { upload: 9, download: 7 } : null },
    discrepancy: {
      upload: { direction: 'Equal', magnitude: '0' },
      download: { direction: 'Equal', magnitude: '0' },
    },
  }
}

test('ordered summary channels clear retirement, accept same-revision freshness, and reset new sessions', async ({
  onTestFinished,
}) => {
  let channel: Channel<TrafficSummaryFrame> | undefined
  let index = 0
  const unsubscribed: number[] = []
  mockIPC((command, args) => {
    if (command === 'subscribe_traffic_summary') {
      channel = (args as { onFrame: Channel<TrafficSummaryFrame> }).onFrame
      return 42
    }
    if (command === 'unsubscribe_traffic_subscription') {
      unsubscribed.push((args as { id: number }).id)
      return null
    }
    if (command === 'get_clash_ws_snapshot')
      return {
        sequence: 0,
        state: 'disconnected',
        recording: { logs: true, memory: true },
        logs: [],
        memory: [],
      }
    if (command === 'plugin:event|listen') return 1
    if (command === 'plugin:event|unlisten') return null
    throw new Error(`Unexpected command: ${command}`)
  })
  let current: ReturnType<typeof useClashWSHistory<'connections'>> = []
  let status: ReturnType<typeof useClashWSStatus> | undefined
  function Consumer() {
    current = useClashWSHistory('connections')
    status = useClashWSStatus()
    return null
  }
  const screen = await render(
    <ClashWSProvider>
      <Consumer />
    </ClashWSProvider>,
  )
  onTestFinished(async () => {
    await screen.unmount()
    clearMocks()
  })
  await expect.poll(() => channel).toBeDefined()
  function send(value: TrafficSummary | null, error: string | null = null) {
    // Deliver through the real per-webview Tauri Channel callback ordering.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(window as any).__TAURI_INTERNALS__.runCallback(channel!.id, {
      index: index++,
      message: { summary: value, error },
    })
  }
  send(summary('a', '9007199254740993'))
  await expect.poll(() => current.at(-1)?.uploadSpeed).toBe(9)
  send(summary('a', '9007199254740992'))
  await expect.poll(() => current.length).toBe(1)
  send(summary('a', '9007199254740993', false))
  await expect.poll(() => current.at(-1)?.memberRates.proxy.upload).toBe(0)
  expect(status?.state).toBe('disconnected')
  send(summary('b', '1'))
  await expect.poll(() => current.length).toBe(1)
  await expect.poll(() => status?.state).toBe('connected')
  send(null, 'retired')
  await expect.poll(() => current.length).toBe(0)
  await expect.poll(() => status?.error).toBe('retired')
  await screen.unmount()
  await expect.poll(() => unsubscribed).toEqual([42])
})

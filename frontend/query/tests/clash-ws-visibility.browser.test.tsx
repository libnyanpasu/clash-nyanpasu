import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { createRpcClient } from '@nyanpasu/rpc'
import type { ClashWsEvent, ClashWsSnapshot } from '@nyanpasu/rpc/types'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import {
  ClashWSProvider,
  useClashWSHistory,
} from '../src/provider/clash-ws-provider'
import { RpcProvider } from '../src/provider/rpc-provider'

// The interface picks the Tauri event transport at import time.
vi.hoisted(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {}, isTauri: true })
})

const snapshot: ClashWsSnapshot = {
  sequence: 0,
  state: 'connected',
  recording: { connections: true, logs: true, traffic: true, memory: true },
  connections: [],
  traffic: [],
  memory: [],
}

const sample = (sequence: number): ClashWsEvent => ({
  sequence,
  update: { kind: 'traffic_updated', data: { up: sequence, down: 0 } },
})

test('a hidden window keeps the ws history without re-rendering', async ({
  onTestFinished,
}) => {
  let snapshots = 0
  mockIPC(
    (wireCommand, args) => {
      expect(wireCommand).toBe('call_rpc')
      expect((args as { method: string }).method).toBe('get_clash_ws_snapshot')
      snapshots += 1
      return snapshot
    },
    { shouldMockEvents: true },
  )
  let visibility: DocumentVisibilityState = 'visible'
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    get: () => visibility,
  })
  let renders = 0
  function Traffic() {
    const traffic = useClashWSHistory('traffic')
    renders += 1
    return <span>{traffic.map((item) => item.up).join(',')}</span>
  }
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  const rpc = createRpcClient()
  onTestFinished(async () => {
    root.unmount()
    await Promise.resolve()
    rpc.dispose()
    await Promise.resolve()
    container.remove()
    clearMocks()
    delete (document as { visibilityState?: unknown }).visibilityState
  })
  root.render(
    <RpcProvider rpc={rpc}>
      <ClashWSProvider>
        <Traffic />
      </ClashWSProvider>
    </RpcProvider>,
  )
  // The provider requests the snapshot once it listens.
  await expect.poll(() => snapshots).toBe(1)
  await emit('clash-ws-event', sample(1))
  await expect.poll(() => container.textContent).toBe('1')

  visibility = 'hidden'
  document.dispatchEvent(new Event('visibilitychange'))
  const before = renders
  for (let sequence = 2; sequence <= 6; sequence++) {
    await emit('clash-ws-event', sample(sequence))
  }
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(renders).toBe(before)

  visibility = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  await expect.poll(() => container.textContent).toBe('1,2,3,4,5,6')
  expect(renders).toBe(before + 1)
})

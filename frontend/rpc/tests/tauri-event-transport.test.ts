import { afterEach, expect, test, vi } from 'vitest'
import { createTauriEventTransport } from '../src/tauri-event-transport'

const native = vi.hoisted(() => ({ listen: vi.fn(), emit: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => native)
afterEach(() => vi.resetAllMocks())

test('propagates native registration failures to the subscriber', async () => {
  native.listen.mockRejectedValueOnce(new Error('window closed'))
  const transport = createTauriEventTransport()
  await expect(transport.listen('event', vi.fn())).rejects.toThrow(
    'window closed',
  )
  transport.dispose()
})

test('disposal releases a pending registration when it completes', async () => {
  let registered!: (stop: () => void) => void
  native.listen.mockImplementationOnce(
    () =>
      new Promise<() => void>((resolve) => {
        registered = resolve
      }),
  )
  const transport = createTauriEventTransport()
  const callback = vi.fn()
  const pending = transport.listen('event', callback)
  transport.dispose()
  const nativeStop = vi.fn()
  registered(nativeStop)
  const stop = await pending
  native.listen.mock.calls[0][1]({ event: 'event', id: 1, payload: 'late' })
  expect(callback).not.toHaveBeenCalled()
  expect(nativeStop).toHaveBeenCalledOnce()
  stop()
  transport.dispose()
  expect(nativeStop).toHaveBeenCalledOnce()
  await transport.listen('event', callback)
  expect(native.listen).toHaveBeenCalledOnce()
})

test('one-shot events unsubscribe exactly once even when the callback throws', async () => {
  const nativeStop = vi.fn()
  native.listen.mockResolvedValueOnce(nativeStop)
  const transport = createTauriEventTransport()
  const callback = vi.fn(() => {
    throw new Error('callback failed')
  })
  const stop = await transport.once('event', callback)
  const handler = native.listen.mock.calls[0][1]
  expect(() => handler({ event: 'event', id: 1, payload: 'first' })).toThrow(
    'callback failed',
  )
  handler({ event: 'event', id: 2, payload: 'second' })
  stop()
  transport.dispose()
  expect(callback).toHaveBeenCalledOnce()
  expect(nativeStop).toHaveBeenCalledOnce()
})

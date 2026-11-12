import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { RpcProvider } from '@nyanpasu/query'
import {
  createIconTransport,
  createRpcClient,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import { CacheImage, TrayImage, WidgetImage } from '../src/components/ui/image'

test('icons_render_transport_urls_without_command_invocations', async ({
  onTestFinished,
}) => {
  const invoke = vi.fn(async () => {
    throw new Error('unexpected icon command')
  })
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  const rpc = createRpcClient({
    commands: { invoke },
    events,
    icons: createIconTransport(false),
  })
  const view = await render(
    <RpcProvider rpc={rpc}>
      <CacheImage icon="https://icons.example/a?token=a+b" alt="cached" />
      <TrayImage mode="tun" version={2} alt="tray" />
      <WidgetImage mode="normal" version={3} alt="widget" />
      <CacheImage icon="data:image/png;base64,AA==" alt="inline" />
    </RpcProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    rpc.dispose()
  })

  expect(document.querySelector('img[alt="cached"]')?.getAttribute('src')).toBe(
    '/bridge/icons/cached?url=https%3A%2F%2Ficons.example%2Fa%3Ftoken%3Da%2Bb',
  )
  expect(document.querySelector('img[alt="tray"]')?.getAttribute('src')).toBe(
    '/bridge/icons/tray?mode=tun&version=2',
  )
  expect(document.querySelector('img[alt="widget"]')?.getAttribute('src')).toBe(
    '/bridge/icons/widget?mode=normal&version=3',
  )
  expect(document.querySelector('img[alt="inline"]')?.getAttribute('src')).toBe(
    'data:image/png;base64,AA==',
  )
  expect(invoke).not.toHaveBeenCalled()
})

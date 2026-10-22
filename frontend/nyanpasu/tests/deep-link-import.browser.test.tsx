import { createRoot } from 'react-dom/client'
import { expect, test, vi, type TestContext } from 'vitest'
import { useDeepLinkImport } from '../src/hooks/use-deep-link-import'

const backend = vi.hoisted(() => ({
  listen: vi.fn<(onPoke: () => void) => Promise<() => void>>(),
  take: vi.fn<() => Promise<unknown>>(),
  importUrl: vi.fn<(input: unknown) => Promise<string | null>>(),
  message: vi.fn(async () => {}),
}))
vi.mock('@/components/profile-import/import-url-provider', () => ({
  useImportUrl: () => backend.importUrl,
}))
vi.mock('@/services/rpc', () => ({
  commands: { takePendingDeepLinks: backend.take },
  events: { schemeRequestReceivedEvent: { listen: backend.listen } },
}))
vi.mock('@/utils/notification', () => ({ message: backend.message }))

const link = (name: string) =>
  `clash://install-config?url=${encodeURIComponent(`https://example.com/${name}`)}`

const imported = (name: string) => ({
  url: `https://example.com/${name}`,
  name: null,
  option: null,
})

function Importer() {
  useDeepLinkImport()
  return null
}

function mount(onTestFinished: TestContext['onTestFinished']) {
  const root = createRoot(document.createElement('div'))
  root.render(<Importer />)
  onTestFinished(() => {
    root.unmount()
    vi.resetAllMocks()
  })
}

test('a listener that fails to register frees the registration and still imports what is queued', async ({
  onTestFinished,
}) => {
  backend.listen.mockRejectedValue(new Error('no event bridge'))
  backend.take.mockResolvedValue({ status: 'ok', data: [link('a')] })
  backend.importUrl.mockResolvedValue('a')

  mount(onTestFinished)
  await expect
    .poll(() => backend.importUrl.mock.calls)
    .toEqual([[imported('a')]])
  expect(backend.take).toHaveBeenCalledOnce()

  // With the registration freed, the next mount registers again.
  mount(onTestFinished)
  await expect.poll(() => backend.listen.mock.calls.length).toBe(2)
})

test('a link queued twice in one batch is imported once', async ({
  onTestFinished,
}) => {
  backend.listen.mockResolvedValue(() => {})
  backend.take.mockResolvedValue({
    status: 'ok',
    data: [link('a'), link('a'), link('b')],
  })
  backend.importUrl.mockResolvedValue('a')

  mount(onTestFinished)
  // One link at a time, so `b` comes only after every earlier one.
  await expect
    .poll(() => backend.importUrl.mock.calls)
    .toEqual([[imported('a')], [imported('b')]])
})

test('a cancelled import reports nothing and lets the next link through', async ({
  onTestFinished,
}) => {
  backend.listen.mockResolvedValue(() => {})
  backend.take.mockResolvedValue({ status: 'ok', data: [link('a'), link('b')] })
  backend.importUrl.mockResolvedValueOnce(null).mockResolvedValueOnce('b')

  mount(onTestFinished)
  await expect
    .poll(() => backend.importUrl.mock.calls)
    .toEqual([[imported('a')], [imported('b')]])
  // Only `b`, which was imported, is reported.
  await expect.poll(() => backend.message.mock.calls.length).toBe(1)
})

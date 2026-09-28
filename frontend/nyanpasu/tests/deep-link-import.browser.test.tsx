import { createRoot } from 'react-dom/client'
import { expect, test, vi, type TestContext } from 'vitest'
import { useDeepLinkImport } from '../src/hooks/use-deep-link-import'

const backend = vi.hoisted(() => ({
  listen: vi.fn<(onPoke: () => void) => Promise<() => void>>(),
  take: vi.fn<() => Promise<unknown>>(),
  create: vi.fn<(input: unknown) => Promise<void>>(),
}))
vi.mock('@nyanpasu/interface', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/interface')>()),
  commands: { takePendingDeepLinks: backend.take },
  events: { schemeRequestReceivedEvent: { listen: backend.listen } },
  useProfile: () => ({ create: { mutateAsync: backend.create } }),
}))
vi.mock('@/utils/notification', () => ({ message: async () => {} }))

const link = (name: string) =>
  `clash://install-config?url=${encodeURIComponent(`https://example.com/${name}`)}`

const imported = (name: string) => ({
  type: 'url',
  data: { url: `https://example.com/${name}`, name: null, option: null },
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
  backend.create.mockResolvedValue()

  mount(onTestFinished)
  await expect.poll(() => backend.create.mock.calls).toEqual([[imported('a')]])
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
  backend.create.mockResolvedValue()

  mount(onTestFinished)
  // One link at a time, so `b` comes only after every earlier one.
  await expect
    .poll(() => backend.create.mock.calls)
    .toEqual([[imported('a')], [imported('b')]])
})

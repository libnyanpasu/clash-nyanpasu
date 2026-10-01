import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { useCurrentProfile } from '../src/pages/(editor)/editor/_modules/hooks'

test('the editor model path survives a profiles refetch', async ({
  onTestFinished,
}) => {
  let fetches = 0
  mockIPC((wireCommand, args) => {
    expect(wireCommand).toBe('call_rpc')
    const { method } = args as { method: string }
    expect(method).toBe('get_profiles')
    fetches += 1
    return {
      current: [],
      items: [
        {
          type: 'config',
          uid: 'p1',
          name: `profile ${fetches}`,
          config: { type: 'file', source: { type: 'local' } },
        },
      ],
    }
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const paths: (string | undefined)[] = []
  function Probe() {
    const { data } = useCurrentProfile('p1')
    paths.push(data?.virtualPath)
    return <span>{data?.name}</span>
  }
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
      <Probe />
    </QueryClientProvider>,
  )
  await expect.poll(() => container.textContent).toBe('profile 1')
  await queries.refetchQueries()
  await expect.poll(() => container.textContent).toBe('profile 2')
  const seen = new Set(paths.filter(Boolean))
  expect(seen.size).toBe(1)
  expect([...seen][0]).toMatch(/\.clash\.yaml$/)
})

import { Suspense } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import useWindowMaximized from '@/hooks/use-window-maximized'
import { QueryClient } from '@tanstack/react-query'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

const nextFrame = () => new Promise<void>((resolve) => setTimeout(resolve, 16))

test('a window drag asks for the maximized state once it settles', async ({
  onTestFinished,
}) => {
  const queries = new QueryClient()
  let renders = 0
  const Consumer = () => {
    const { isMaximized } = useWindowMaximized()
    renders += 1
    return <span>{String(isMaximized)}</span>
  }

  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
  })
  root.render(
    <QueryClientProvider client={queries}>
      <Suspense>
        <Consumer />
      </Suspense>
    </QueryClientProvider>,
  )
  await expect.poll(() => container.textContent).toBe('false')

  const fetches = () =>
    queries.getQueryCache().find({ queryKey: ['isMaximized'] })!.state
      .dataUpdateCount
  const fetchesBefore = fetches()
  const rendersBefore = renders

  // A window drag fires a resize event every frame. The frames are timers, not
  // animation frames: a throttled runner may space those beyond the debounce,
  // while a stalled timer still runs before the later debounce deadline.
  for (let i = 0; i < 10; i++) {
    window.dispatchEvent(new Event('resize'))
    await nextFrame()
  }
  await new Promise((resolve) => setTimeout(resolve, 400))

  expect(fetches() - fetchesBefore).toBe(1)
  // The state did not change, so the consumer has nothing to re-render for.
  expect(renders - rendersBefore).toBe(0)
})

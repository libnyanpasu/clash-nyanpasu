// tsconfig.test.json does not load the svgr client types the logo import needs.
/// <reference types="../node_modules/vite-plugin-svgr/client" />
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import AnimatedLogo from '@/components/logo/animated-logo'

// Browser tests do not run svgr.
vi.mock('@/assets/image/logo.svg?react', () => ({ default: () => null }))

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

test('the indeterminate sway runs as a browser animation', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<AnimatedLogo indeterminate />)

  // Let the 1 s entrance finish before watching the sway.
  await sleep(1300)

  let styleWrites = 0
  const observer = new MutationObserver(() => (styleWrites += 1))
  observer.observe(container, {
    attributeFilter: ['style'],
    subtree: true,
  })
  onTestFinished(() => observer.disconnect())
  await sleep(500)

  expect(styleWrites).toBe(0)
  const running = container
    .querySelector('[data-slot="app-header-logo"]')!
    .getAnimations({ subtree: true })
    .filter((animation) => animation.playState === 'running')
  expect(running).toHaveLength(1)
})

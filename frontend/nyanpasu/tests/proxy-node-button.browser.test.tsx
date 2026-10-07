import { expect, test, vi } from 'vitest'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProxyNodeButton from '@/pages/(main)/main/proxies/group/_modules/proxy-node-button'
import type { ClashProxiesQueryProxyItem } from '@nyanpasu/query'

const proxy: ClashProxiesQueryProxyItem = {
  name: 'node-a',
  type: 'Vless',
  udp: true,
  history: [],
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
}

async function renderButton(
  selectable: boolean,
  fixed = false,
  node: ClashProxiesQueryProxyItem = proxy,
  extra: { delay?: number; leaf?: string } = {},
) {
  const onSelect = vi.fn(async () => {})
  const onDelayTest = vi.fn(async () => {})
  const screen = await render(
    <BlockTaskProvider>
      <TooltipProvider>
        <ProxyNodeButton
          proxy={node}
          selectable={selectable}
          fixed={fixed}
          onSelect={onSelect}
          onDelayTest={onDelayTest}
          history={node.history}
          delay={extra.delay}
          leaf={extra.leaf}
        />
      </TooltipProvider>
    </BlockTaskProvider>,
  )
  return { screen, onSelect, onDelayTest }
}

test('a node of a selectable group is selected on click', async () => {
  const { screen, onSelect } = await renderButton(true)
  const card = screen.container.querySelector('[data-selectable="true"]')
  expect(card).not.toBeNull()
  expect(card!.getAttribute('aria-disabled')).not.toBe('true')
  await screen.getByText('node-a').click()
  await expect.poll(() => onSelect).toHaveBeenCalledWith(proxy)
})

test('a node of a group the core selects on its own ignores clicks but still tests', async () => {
  const { screen, onSelect, onDelayTest } = await renderButton(false)
  const card = screen.container.querySelector<HTMLElement>(
    '[data-selectable="false"]',
  )
  expect(card).not.toBeNull()
  expect(card!.getAttribute('aria-disabled')).toBe('true')
  // Playwright refuses to click an aria-disabled element, so click the DOM node.
  card!.click()
  // The card is the button; the latency control is the `asChild` span nested
  // in it (no history yet, so no delay chip).
  const delayControl = card!.querySelector<HTMLElement>('span[data-loading]')!
  delayControl.click()
  await expect.poll(() => onDelayTest).toHaveBeenCalledWith(proxy)
  expect(onSelect).not.toHaveBeenCalled()
})

test('only the pinned member shows the pin', async () => {
  const pinned = await renderButton(true, true)
  expect(
    pinned.screen.container.querySelector(
      '[data-slot="proxy-node-fixed-icon"]',
    ),
  ).not.toBeNull()
  await pinned.screen.unmount()

  const other = await renderButton(true, false)
  expect(
    other.screen.container.querySelector('[data-slot="proxy-node-fixed-icon"]'),
  ).toBeNull()
})

test('the delay chip shows the delay the page computed', async () => {
  const { screen } = await renderButton(true, false, proxy, { delay: 77 })
  await expect
    .poll(
      () =>
        screen.container.querySelector('[data-slot="proxy-node-delay"]')
          ?.textContent,
    )
    .toBe('77 ms')
})

test('a nested group member shows its leaf node', async () => {
  const { screen } = await renderButton(true, false, proxy, {
    delay: 42,
    leaf: 'HK-01',
  })
  const leaf = () =>
    screen.container.querySelector('[data-slot="proxy-node-leaf"]')
  await expect.poll(() => leaf()?.textContent).toBe('→ HK-01')
  expect(leaf()?.getAttribute('title')).toBe('HK-01')
  expect(
    screen.container.querySelector('[data-slot="proxy-node-delay"]')
      ?.textContent,
  ).toBe('42 ms')
})

test('a long leaf truncates instead of clipping the feature chips', async () => {
  const screen = await render(
    <BlockTaskProvider>
      <TooltipProvider>
        <div style={{ width: 300 }}>
          <ProxyNodeButton
            proxy={{ ...proxy, type: 'URLTest' }}
            selectable
            fixed={false}
            onSelect={async () => {}}
            onDelayTest={async () => {}}
            history={proxy.history}
            delay={39}
            leaf="香港标准 IEPL 专线 6 [Premium] 0.8x"
          />
        </div>
      </TooltipProvider>
    </BlockTaskProvider>,
  )
  const features = screen.container.querySelector<HTMLElement>(
    '[data-slot="proxy-node-features"]',
  )!
  await expect.poll(() => features.clientWidth).toBeGreaterThan(0)
  expect(features.scrollWidth).toBeLessThanOrEqual(features.clientWidth)
  const leaf = screen.container.querySelector<HTMLElement>(
    '[data-slot="proxy-node-leaf"]',
  )!
  expect(leaf.scrollWidth).toBeGreaterThan(leaf.clientWidth)
})

test('a plain member shows no leaf', async () => {
  const { screen } = await renderButton(true, false, proxy, { delay: 42 })
  await expect
    .poll(
      () =>
        screen.container.querySelector('[data-slot="proxy-node-delay"]')
          ?.textContent,
    )
    .toBe('42 ms')
  expect(
    screen.container.querySelector('[data-slot="proxy-node-leaf"]'),
  ).toBeNull()
})

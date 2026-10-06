import { expect, test, vi } from 'vitest'
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

async function renderButton(selectable: boolean, fixed = false) {
  const onSelect = vi.fn(async () => {})
  const onDelayTest = vi.fn(async () => {})
  const screen = await render(
    <BlockTaskProvider>
      <TooltipProvider>
        <ProxyNodeButton
          proxy={proxy}
          selectable={selectable}
          fixed={fixed}
          onSelect={onSelect}
          onDelayTest={onDelayTest}
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
  pinned.screen.unmount()

  const other = await renderButton(true, false)
  expect(
    other.screen.container.querySelector('[data-slot="proxy-node-fixed-icon"]'),
  ).toBeNull()
})

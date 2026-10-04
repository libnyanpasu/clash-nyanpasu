// Tailwind gives the rows their transitions and the highlight its keyframes.
import '@/assets/styles/tailwind.css'
import type { ComponentProps } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import type { ClashRule } from '@nyanpasu/rpc/types'
import { RuleRow } from '../src/pages/(main)/main/rules/_modules/rule-row'

const LABEL = 'DomainSuffix,google.com'

const rule = {
  type: 'DomainSuffix',
  payload: 'google.com',
  proxy: 'Proxy',
} as ClashRule

const live = { connections: 3, downloadSpeed: 1024, uploadSpeed: 512 }

const total = { download: 4096, upload: 2048 }

const renderRow = (props: Partial<ComponentProps<typeof RuleRow>> = {}) =>
  render(
    <TooltipProvider>
      <RuleRow
        index={1}
        rule={rule}
        label={LABEL}
        live={live}
        total={total}
        search=""
        {...props}
      />
    </TooltipProvider>,
  )

// The visible numbers name the jumps, followed by their actions.
const connectionsName = `3 ${m.rules_view_connections()}`

const usageName = `↓${parseTraffic(4096).join(' ')} ↑${parseTraffic(2048).join(' ')} ${m.rules_view_usage()}`

const slot = (container: HTMLElement, name: string) =>
  container.querySelector<HTMLElement>(`[data-slot="${name}"]`)

test('the live badge and the total jump with the rule label', async () => {
  const onViewConnections = vi.fn()
  const onViewUsage = vi.fn()

  const screen = await renderRow({ onViewConnections, onViewUsage })

  await screen.getByRole('button', { name: connectionsName }).click()
  expect(onViewConnections).toHaveBeenCalledWith(LABEL)

  await screen.getByRole('button', { name: usageName }).click()
  expect(onViewUsage).toHaveBeenCalledWith(LABEL)
})

test('the jumps are named by their numbers, then their actions', async () => {
  const screen = await renderRow({
    onViewConnections: vi.fn(),
    onViewUsage: vi.fn(),
  })

  const connections = slot(screen.container, 'rules-row-view-connections')!
  const usage = slot(screen.container, 'rules-row-view-usage')!

  await expect.element(connections).toHaveAccessibleName(connectionsName)
  await expect.element(usage).toHaveAccessibleName(usageName)

  // Phrasing content only: a button holds no block such as a div.
  expect(usage.querySelector('div')).toBeNull()
})

test('only the focused row fades its highlight slowly', async () => {
  const screen = await renderRow({ focused: true })

  const row = slot(screen.container, 'rules-row')!

  expect(getComputedStyle(row).animationName).toBe('focus-highlight')
  expect(getComputedStyle(row).animationDuration).toBe('2s')
  expect(getComputedStyle(row).transitionDuration).toBe('0.15s')

  await screen.rerender(
    <TooltipProvider>
      <RuleRow
        index={1}
        rule={rule}
        label={LABEL}
        live={live}
        total={total}
        search=""
        focused={false}
      />
    </TooltipProvider>,
  )

  expect(getComputedStyle(row).animationName).toBe('none')
  expect(getComputedStyle(row).transitionDuration).toBe('0.15s')
})

test('without callbacks the cells are not buttons', async () => {
  const screen = await renderRow()

  expect(screen.container.querySelector('button')).toBeNull()
  expect(slot(screen.container, 'rules-row-view-connections')).toBeNull()
  expect(slot(screen.container, 'rules-row-view-usage')).toBeNull()
})

test('an idle rule and an empty total offer no jump', async () => {
  const screen = await renderRow({
    live: { connections: 0, downloadSpeed: 0, uploadSpeed: 0 },
    total: { download: 0, upload: 0 },
    onViewConnections: vi.fn(),
    onViewUsage: vi.fn(),
  })

  expect(slot(screen.container, 'rules-row-view-connections')).toBeNull()
  expect(slot(screen.container, 'rules-row-view-usage')).toBeNull()
})

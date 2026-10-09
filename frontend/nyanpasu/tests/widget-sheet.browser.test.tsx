import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import { DndGrid, DndGridRoot, useDndGridRoot } from '@nyanpasu/ui/dnd-grid'
import ContextMenuProvider, {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import {
  DashboardProvider,
  useDashboardContext,
} from '@/components/widgets/provider'
import { WidgetSheet } from '@/pages/(main)/main/dashboard/_modules/widget-sheet'
import { m } from '@/paraglide/messages'
import { createRpcClient } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'
import { TestQueryProvider } from './query-provider'

// Card data is irrelevant to the overlay lifecycle. Keep the real source grid
// and draggable DOM while rendering one static card without business queries.
vi.mock('@/components/widgets/consts', async (importOriginal) => {
  const original =
    await importOriginal<typeof import('@/components/widgets/consts')>()
  const { DndGridItem } = await import('@nyanpasu/ui/dnd-grid')
  return {
    ...original,
    RENDER_MAP: {
      [original.WidgetId.TrafficDown]: ({ id }: { id: string }) => (
        <DndGridItem id={id} minW={2} minH={2}>
          <div
            data-testid="source-card"
            style={{ width: '100%', height: '100%', background: '#ddd' }}
          >
            Preview card
          </div>
        </DndGridItem>
      ),
    },
  }
})

function Harness({ onPageAction }: { onPageAction: () => void }) {
  const { setOpenSheet } = useDashboardContext()
  const root = useDndGridRoot()
  const [clicks, setClicks] = useState(0)
  const [drops, setDrops] = useState(0)
  return (
    <>
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <div
            data-testid="dashboard"
            style={{ position: 'relative', width: 640, height: 300 }}
          >
            <div
              style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}
            >
              <DndGrid
                gridId="dashboard"
                items={[]}
                disabled={false}
                onExternalDrop={() => setDrops((value) => value + 1)}
              >
                {() => null}
              </DndGrid>
            </div>
            <button onClick={() => setOpenSheet(true)}>Open library</button>
            <button
              data-testid="page-action"
              onClick={() => {
                onPageAction()
                setClicks((value) => value + 1)
              }}
            >
              Page action
            </button>
            <output data-testid="clicks">{clicks}</output>
            <output data-testid="drops">{drops}</output>
            <output data-testid="drag-state">
              {root?.activeDrag ? 'dragging' : 'idle'}
            </output>
          </div>
        </RegisterContextMenuTrigger>
        <RegisterContextMenuContent>
          <ContextMenuItem onSelect={() => setOpenSheet(true)}>
            Add from context menu
          </ContextMenuItem>
        </RegisterContextMenuContent>
      </RegisterContextMenu>
      <WidgetSheet
        onSourceDragStart={() => setOpenSheet(false)}
        onAdd={() => setDrops((value) => value + 1)}
      />
    </>
  )
}

async function mount(onFinished: (callback: () => void) => void) {
  const onPageAction = vi.fn()
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const base = createRpcClient()
  const rpc = new Proxy(base, {
    get(target, key) {
      if (key === 'getStorageItem')
        return async () => ({ status: 'ok', data: null })
      if (key === 'listenResync') return () => () => {}
      if (key === 'events')
        return { storageValueChangedEvent: { listen: async () => () => {} } }
      return Reflect.get(target, key)
    },
  })
  const originalPointerEvents = document.body.style.pointerEvents
  const style = document.createElement('style')
  style.textContent = `[data-testid=dashboard] { max-width:100vw; }
    [data-testid=dashboard] [data-slot=dnd-grid-container] { width:100%; height:100%; }
    [role=menu] { background:white; min-width:200px; }`
  document.head.append(style)
  const view = await render(
    <TestQueryProvider client={client} rpc={rpc}>
      <DashboardProvider>
        <ContextMenuProvider>
          <div>
            <DndGridRoot>
              <Harness onPageAction={onPageAction} />
            </DndGridRoot>
          </div>
        </ContextMenuProvider>
      </DashboardProvider>
    </TestQueryProvider>,
  )
  onFinished(() => {
    view.unmount()
    client.clear()
    style.remove()
    document.body.style.pointerEvents = originalPointerEvents
  })

  return onPageAction
}

test('closing a library opened by the context menu restores page clicks', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished)
  await userEvent.click(page.getByTestId('dashboard'), { button: 'right' })
  await userEvent.click(
    page.getByRole('menuitem', { name: 'Add from context menu' }),
  )
  await expect.element(page.getByRole('dialog')).toBeVisible()
  await userEvent.click(
    page.getByRole('button', {
      name: m.dashboard_widget_config_close_library(),
    }),
  )
  await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
  await expect.poll(() => document.body.style.pointerEvents).not.toBe('none')
  await userEvent.click(page.getByRole('button', { name: 'Page action' }))
  await expect.element(page.getByTestId('clicks')).toHaveTextContent('1')
})

test('dragging a source out closes the library and restores page clicks', async ({
  onTestFinished,
}) => {
  const onPageAction = await mount(onTestFinished)
  await userEvent.click(page.getByRole('button', { name: 'Open library' }))
  await expect.element(page.getByTestId('source-card')).toBeVisible()
  await expect
    .poll(() => {
      const drawer = document.querySelector<HTMLElement>(
        '[data-slot=drawer-content]',
      )!
      return new DOMMatrix(getComputedStyle(drawer).transform).m42
    })
    .toBeLessThan(1)
  // The modal overlay covers the destination until the first drag movement
  // closes the library. Move there without waiting for target actionability.
  await page
    .getByTestId('source-card')
    .dropTo(page.getByTestId('page-action'), {
      force: true,
      sourcePosition: { x: 80, y: 50 },
    })
  await expect.element(page.getByTestId('drops')).toHaveTextContent('1')
  await expect.element(page.getByTestId('drag-state')).toHaveTextContent('idle')
  await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
  await expect.poll(() => document.body.style.pointerEvents).not.toBe('none')
  // The drag sensor briefly suppresses click events to avoid clicking the
  // drop target. Verify ordinary clicks recover once that suppression expires.
  await expect
    .poll(
      async () => {
        if (onPageAction.mock.calls.length === 0) {
          await userEvent.click(
            page.getByRole('button', { name: 'Page action' }),
          )
        }
        return onPageAction.mock.calls.length
      },
      { timeout: 5000 },
    )
    .toBe(1)
  await expect.element(page.getByTestId('clicks')).toHaveTextContent('1')
})

test('search filters localized widget names and preserves add actions', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished)
  await userEvent.click(page.getByRole('button', { name: 'Open library' }))

  const drawer = page.getByRole('dialog')
  const search = page.getByRole('searchbox', {
    name: m.dashboard_widget_library_search(),
  })
  await expect.element(search).toBeVisible()
  const drawerElement = document.querySelector<HTMLElement>(
    '[data-slot=drawer-content]',
  )!
  await expect
    .poll(() => drawerElement.getBoundingClientRect().width)
    .toBe(Math.min(window.innerWidth - 16, 960))
  await expect
    .poll(() => drawerElement.getBoundingClientRect().height)
    .toBeCloseTo(window.innerHeight * 0.85, 0)

  await userEvent.type(search, m.dashboard_widget_traffic_download())
  await expect.element(page.getByTestId('source-card')).toBeVisible()
  await expect
    .element(page.getByRole('button', { name: m.dashboard_add_widget() }))
    .toBeVisible()

  await userEvent.type(search, ' no match')
  await expect.element(page.getByTestId('source-card')).not.toBeInTheDocument()
  await expect
    .element(page.getByText(m.dashboard_widget_library_no_results()))
    .toBeVisible()

  await userEvent.click(
    page.getByRole('button', {
      name: m.dashboard_widget_library_search_clear(),
    }),
  )
  await expect.element(search).toHaveValue('')
  await expect.element(page.getByTestId('source-card')).toBeVisible()

  await userEvent.click(
    page.getByRole('button', { name: m.dashboard_add_widget() }),
  )
  await expect.element(page.getByTestId('drops')).toHaveTextContent('1')
  await expect.element(drawer).toBeVisible()
})

test('narrow viewports keep the drawer and widget preview inside the screen', async ({
  onTestFinished,
}) => {
  await page.viewport(900, 600)
  await mount(onTestFinished)
  onTestFinished(async () => page.viewport(900, 600))
  await userEvent.click(page.getByRole('button', { name: 'Open library' }))

  const drawer = document.querySelector<HTMLElement>(
    '[data-slot=drawer-content]',
  )!
  const search = page.getByRole('searchbox', {
    name: m.dashboard_widget_library_search(),
  })
  const heading = page.getByRole('heading', { name: m.dashboard_add_widget() })

  for (const [width, height] of [
    [320, 640],
    [384, 720],
  ]) {
    await page.viewport(width, height)
    await expect.element(search).toBeVisible()
    await expect.element(heading).toBeVisible()
    await expect.element(page.getByTestId('source-card')).toBeVisible()
    await expect
      .poll(() => drawer.getBoundingClientRect().width)
      .toBe(width - 16)
    await expect
      .poll(() => drawer.getBoundingClientRect().height)
      .toBeCloseTo(height * 0.85, 0)
    await expect
      .poll(() => drawer.getBoundingClientRect().bottom)
      .toBeCloseTo(height, 0)

    const drawerRect = drawer.getBoundingClientRect()
    const grid = drawer.querySelector<HTMLElement>(
      '[data-slot=dnd-grid-container]',
    )!
    const gridRect = grid.getBoundingClientRect()
    const card = page
      .getByTestId('source-card')
      .element()
      .closest('[role=button]')!
    const cardRect = card.getBoundingClientRect()
    const addButton = drawer.querySelector<HTMLButtonElement>(
      'button[aria-label="Add Widget"]',
    )!
    const addRect = addButton.getBoundingClientRect()

    expect(drawerRect.left).toBeGreaterThanOrEqual(0)
    expect(drawerRect.right).toBeLessThanOrEqual(width)
    expect(drawerRect.bottom).toBeCloseTo(height, 0)
    expect(cardRect.left).toBeGreaterThanOrEqual(gridRect.left - 1)
    expect(cardRect.right).toBeLessThanOrEqual(gridRect.right + 1)
    expect(addRect.left).toBeGreaterThanOrEqual(gridRect.left - 1)
    expect(addRect.right).toBeLessThanOrEqual(gridRect.right + 1)
    expect(drawer.scrollWidth).toBeLessThanOrEqual(drawer.clientWidth)
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(width)
    expect(document.documentElement.scrollHeight).toBeLessThanOrEqual(height)
  }
})

test('repeated Escape and outside dismissal leave the page interactive', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished)
  for (const dismiss of ['escape', 'outside', 'escape', 'outside']) {
    await userEvent.click(page.getByRole('button', { name: 'Open library' }))
    await expect.element(page.getByRole('dialog')).toBeVisible()
    if (dismiss === 'escape') {
      await userEvent.keyboard('{Escape}')
    } else {
      await userEvent.click(
        document.querySelector<HTMLElement>('[data-slot=drawer-overlay]')!,
        { position: { x: window.innerWidth - 10, y: 10 } },
      )
    }
    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
    await expect.poll(() => document.body.style.pointerEvents).not.toBe('none')
    await userEvent.click(page.getByRole('button', { name: 'Page action' }))
  }
  await expect.element(page.getByTestId('clicks')).toHaveTextContent('4')
})

test('the drawer slides out and the overlay fades before unmounting', async ({
  onTestFinished,
}) => {
  await mount(onTestFinished)
  await userEvent.click(page.getByRole('button', { name: 'Open library' }))
  await expect
    .poll(() => {
      const drawer = document.querySelector<HTMLElement>(
        '[data-slot=drawer-content]',
      )!
      return new DOMMatrix(getComputedStyle(drawer).transform).m42
    })
    .toBeLessThan(1)

  const drawer = document.querySelector<HTMLElement>(
    '[data-slot=drawer-content]',
  )!
  const overlay = document.querySelector<HTMLElement>(
    '[data-slot=drawer-overlay]',
  )!
  const observed = {
    closed: false,
    inert: false,
    hidden: false,
    slide: false,
    fade: false,
  }
  let frame = 0

  // Observe inside browser frames before the remote click returns. The two
  // animations can advance on different frames and finish before that round trip.
  const observeExit = () => {
    if (drawer.dataset.state === 'closed') {
      observed.closed = true
      observed.inert ||= drawer.inert
      observed.hidden ||= drawer.getAttribute('aria-hidden') === 'true'
      if (drawer.isConnected) {
        observed.slide ||=
          new DOMMatrix(getComputedStyle(drawer).transform).m42 > 1
      }
      if (overlay.isConnected) {
        observed.fade ||= Number(getComputedStyle(overlay).opacity) < 1
      }
    }
    if (drawer.isConnected || overlay.isConnected) {
      frame = requestAnimationFrame(observeExit)
    }
  }
  frame = requestAnimationFrame(observeExit)
  onTestFinished(() => cancelAnimationFrame(frame))

  await userEvent.click(
    page.getByRole('button', {
      name: m.dashboard_widget_config_close_library(),
    }),
  )
  await expect.poll(() => drawer.isConnected || overlay.isConnected).toBe(false)
  expect(observed).toEqual({
    closed: true,
    inert: true,
    hidden: true,
    slide: true,
    fade: true,
  })
  await expect.poll(() => document.body.style.pointerEvents).not.toBe('none')
  await userEvent.click(page.getByRole('button', { name: 'Page action' }))
  await expect.element(page.getByTestId('clicks')).toHaveTextContent('1')
})

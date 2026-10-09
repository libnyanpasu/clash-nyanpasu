import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { act, useEffect, type PropsWithChildren } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, onTestFinished, test, vi } from 'vitest'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import {
  DashboardProvider,
  useDashboardContext,
} from '@/components/widgets/provider'
import { WidgetId } from '@/components/widgets/widget-config'
import { Route as DashboardRoute } from '@/pages/(main)/main/dashboard'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'
import { TestQueryProvider } from './query-provider'

beforeEach(() => vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@/components/widgets/consts', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('@/components/widgets/consts')>()
  const { DndGridItem: GridItem } = await import('@nyanpasu/ui/dnd-grid')
  const RENDER_MAP = Object.fromEntries(
    Object.entries(actual.RENDER_MAP).map(([type, _Widget]) => {
      const Widget = ({ id }: { id: string }) => {
        const minimum = actual.WIDGET_MIN_SIZE_MAP[type as WidgetId]
        return (
          <GridItem id={id} {...minimum}>
            <div data-layout-widget={id} data-layout-type={type}>
              {id}
            </div>
          </GridItem>
        )
      }
      return [type, Widget]
    }),
  ) as typeof actual.RENDER_MAP

  return { ...actual, RENDER_MAP }
})

vi.mock('@/components/widgets/widget-config-menu', () => ({
  default: () => null,
  WidgetConfigSaveStatus: () => null,
}))

type StoredItem = {
  id: string
  type: WidgetId
  x: number
  y: number
  w: number
  h: number
}

const smallLayout: StoredItem[] = [
  {
    id: 'small-down',
    type: WidgetId.TrafficDown,
    x: 0,
    y: 0,
    w: 2,
    h: 2,
  },
  {
    id: 'small-up',
    type: WidgetId.TrafficUp,
    x: 2,
    y: 0,
    w: 2,
    h: 2,
  },
  {
    id: 'small-memory',
    type: WidgetId.Memory,
    x: 0,
    y: 2,
    w: 2,
    h: 2,
  },
]

const largerLayout: StoredItem[] = [
  {
    id: 'large-connections',
    type: WidgetId.Connections,
    x: 0,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: 'large-proxy',
    type: WidgetId.ProxyMode,
    x: 4,
    y: 4,
    w: 4,
    h: 2,
  },
]

function createRpcHarness(legacyStorage: Record<string, StoredItem[]>) {
  const writes: { key: string; value: unknown }[] = []
  const events: RpcEventTransport = {
    listen: async () => () => {},
    once: async () => () => {},
    emit: async () => {},
    listenMutation: async () => () => {},
    listenResync: () => () => {},
    dispose: () => {},
  }
  const rpc = createRpcClient({
    commands: {
      invoke: async <T,>(method: string, params?: Record<string, unknown>) => {
        if (method === 'get_storage_item') {
          const key = params?.key
          return (
            key === 'dashboard-widgets' ? JSON.stringify(legacyStorage) : null
          ) as T
        }
        if (method === 'set_storage_item') {
          const key = String(params?.key)
          const serialized = String(params?.value)
          writes.push({ key, value: JSON.parse(serialized) as unknown })
          return null as T
        }
        throw new Error(`Unexpected RPC method: ${method}`)
      },
    },
    events,
  })

  return { rpc, writes }
}

function EnableDashboardEditing({ children }: PropsWithChildren) {
  const { setIsEditing } = useDashboardContext()

  useEffect(() => setIsEditing(true), [setIsEditing])

  return children
}

function layoutWidgetIds(container: HTMLElement) {
  return [...container.querySelectorAll<HTMLElement>('[data-layout-widget]')]
    .map((widget) => widget.dataset.layoutWidget)
    .sort()
}

test('dashboard keeps its selected source through resize and saves effective bounds after an edit', async () => {
  localStorage.clear()
  const legacyStorage = { '4x5': smallLayout, '8x6': largerLayout }
  const harness = createRpcHarness(legacyStorage)
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchOnWindowFocus: false },
      mutations: { retry: false },
    },
  })
  const container = document.createElement('div')
  container.style.cssText =
    'display:flex;flex-direction:column;width:352px;height:432px;'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(async () => {
    await act(async () => root.unmount())
    container.remove()
    queryClient.clear()
    harness.rpc.dispose()
    localStorage.clear()
  })

  const DashboardPage = DashboardRoute.options.component!
  await act(async () => {
    root.render(
      <TestQueryProvider client={queryClient} rpc={harness.rpc}>
        <DashboardProvider>
          <ContextMenuProvider>
            <EnableDashboardEditing>
              <DashboardPage />
            </EnableDashboardEditing>
          </ContextMenuProvider>
        </DashboardProvider>
      </TestQueryProvider>,
    )
  })

  const widgetIds = () =>
    [...container.querySelectorAll<HTMLElement>('[data-layout-widget]')]
      .map((widget) => widget.dataset.layoutWidget)
      .sort()

  await expect
    .poll(widgetIds)
    .toEqual(['small-down', 'small-memory', 'small-up'])

  const dashboardGrid = container.querySelector<HTMLElement>(
    '[data-slot="dnd-grid-container"]',
  )!
  expect(dashboardGrid).not.toBeNull()
  const expectGridSize = async (width: number, height: number) => {
    await expect
      .poll(() => Math.round(dashboardGrid.getBoundingClientRect().width))
      .toBe(width)
    await expect
      .poll(() => Math.round(dashboardGrid.getBoundingClientRect().height))
      .toBe(height)
  }
  await expectGridSize(320, 400)

  // This size also fits the distinct 8x6 preset. The source selected at 4x5
  // must remain pinned while the browser reports real ResizeObserver changes.
  await act(async () => {
    container.style.width = '656px'
    container.style.height = '512px'
  })
  await expectGridSize(624, 480)
  await expect
    .poll(widgetIds)
    .toEqual(['small-down', 'small-memory', 'small-up'])
  const wideLayout = layoutWidgetIds(container)

  await act(async () => {
    container.style.width = '352px'
    container.style.height = '192px'
  })
  await expectGridSize(320, 160)
  await expect.poll(widgetIds).toEqual(['small-down', 'small-up'])

  await act(async () => {
    container.style.width = '656px'
    container.style.height = '512px'
  })
  await expectGridSize(624, 480)
  await expect
    .poll(widgetIds)
    .toEqual(['small-down', 'small-memory', 'small-up'])
  expect(layoutWidgetIds(container)).toEqual(wideLayout)
  expect(harness.writes).toEqual([])

  const item = container.querySelector<HTMLElement>(
    '[data-layout-widget="small-memory"]',
  )!.parentElement!
  const handle = item.querySelector<HTMLElement>('[data-slot="resize-handle"]')
  expect(handle).not.toBeNull()
  const rect = handle!.getBoundingClientRect()
  const setPointerCapture = vi
    .spyOn(HTMLElement.prototype, 'setPointerCapture')
    .mockImplementation(() => {})
  const hasPointerCapture = vi
    .spyOn(HTMLElement.prototype, 'hasPointerCapture')
    .mockReturnValue(true)
  const releasePointerCapture = vi
    .spyOn(HTMLElement.prototype, 'releasePointerCapture')
    .mockImplementation(() => {})
  onTestFinished(() => {
    setPointerCapture.mockRestore()
    hasPointerCapture.mockRestore()
    releasePointerCapture.mockRestore()
  })
  const dispatchPointer = (
    type: 'pointerdown' | 'pointermove' | 'pointerup',
    clientX: number,
    clientY: number,
  ) =>
    handle!.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        pointerId: 1,
        pointerType: 'mouse',
        isPrimary: true,
        button: 0,
        buttons: type === 'pointerup' ? 0 : 1,
        clientX,
        clientY,
      }),
    )

  await act(async () => {
    dispatchPointer(
      'pointerdown',
      rect.x + rect.width / 2,
      rect.y + rect.height / 2,
    )
  })
  await act(async () => {
    dispatchPointer(
      'pointermove',
      rect.x + rect.width / 2 + 80,
      rect.y + rect.height / 2,
    )
  })
  await act(async () => {
    dispatchPointer(
      'pointerup',
      rect.x + rect.width / 2 + 80,
      rect.y + rect.height / 2,
    )
  })

  await expect.poll(() => harness.writes.length).toBe(1)
  const saved = harness.writes[0]
  expect(saved.key).toBe('dashboard-widgets')
  const migrated = saved.value as {
    version: number
    preferredId: string | null
    entries: {
      id: string
      size: { cols: number; rows: number }
      items: StoredItem[]
    }[]
  }
  expect(migrated.version).toBe(2)
  expect(migrated.preferredId).toBe('legacy:4x5')
  expect(migrated.entries.map(({ id }) => id)).toEqual([
    'legacy:4x5',
    'legacy:8x6',
  ])
  const editedSource = migrated.entries.find(({ id }) => id === 'legacy:4x5')!
  const effectiveBounds = editedSource.items.reduce(
    (bounds, entry) => ({
      cols: Math.max(bounds.cols, entry.x + entry.w),
      rows: Math.max(bounds.rows, entry.y + entry.h),
    }),
    { cols: 1, rows: 1 },
  )
  expect(editedSource.size).toEqual(effectiveBounds)
  expect(editedSource.size).not.toEqual({ cols: 8, rows: 6 })
  expect(editedSource.items.find(({ id }) => id === 'small-memory')?.w).toBe(3)
})

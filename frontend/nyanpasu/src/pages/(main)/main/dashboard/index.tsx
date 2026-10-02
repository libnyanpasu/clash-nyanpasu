import AddRounded from '~icons/material-symbols/add-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { useCallback, useMemo, useRef, useState } from 'react'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  DndGrid,
  DndGridProvider,
  DndGridRoot,
  hasOverlap,
  useDndGridRoot,
  type DndGridItemType,
  type GridSize,
} from '@nyanpasu/ui/dnd-grid'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { DragOverlay } from '@dnd-kit/core'
import { useKvStorage } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import {
  DashboardItem,
  DEFAULT_ITEMS,
  DEFAULT_LAYOUTS,
  LayoutStorage,
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WidgetId,
} from './_modules/consts'
import EditAction from './_modules/edit-action'
import {
  adaptLayout,
  findBestLayout,
  findClosestStoredLayout,
  sizeKey,
} from './_modules/layout-adapt'
import { useDashboardContext } from './_modules/provider'
import { WidgetSheet } from './_modules/widget-sheet'

export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
})

function normalizeItems(items: DndGridItemType<string>[]): DashboardItem[] {
  return items.map((item) => ({
    ...item,
    type: (item as DashboardItem).type ?? (item.id as WidgetId),
  }))
}

// Widgets declare these minimum sizes; they are known before any widget has
// rendered.
const constraintsFor = (items: DashboardItem[]) =>
  Object.fromEntries(
    items.map((item) => [item.id, WIDGET_MIN_SIZE_MAP[item.type]]),
  )

function layoutForSize(storage: LayoutStorage, size: GridSize) {
  const bestLayout = findBestLayout(storage, size)
  if (bestLayout) {
    return normalizeItems(bestLayout)
  }

  const base = normalizeItems(
    findClosestStoredLayout(storage, size) ?? DEFAULT_ITEMS,
  )

  return adaptLayout(base, size, constraintsFor(base))
}

function DashboardDragOverlay({
  displayItems,
}: {
  displayItems: DashboardItem[]
}) {
  const root = useDndGridRoot()
  const activeDrag = root?.activeDrag ?? null

  return (
    <DragOverlay dropAnimation={null}>
      {activeDrag &&
        (() => {
          const widgetType =
            displayItems.find((i) => i.id === activeDrag.itemId)?.type ??
            (activeDrag.itemId as WidgetId)
          const WidgetComponent = RENDER_MAP[widgetType]

          if (!WidgetComponent) {
            return null
          }

          return (
            <div
              className="cursor-grabbing rounded-2xl opacity-90"
              style={{
                width: activeDrag.dims.width,
                height: activeDrag.dims.height,
              }}
            >
              <DndGridProvider
                value={{
                  displayItems: activeDrag.dragIdPrefix
                    ? []
                    : displayItems.filter(
                        (item) => item.id === activeDrag.itemId,
                      ),
                  getItemRect: () => ({
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                  }),
                  dropInfoMap: {},
                  activeItemId: null,
                  resizingItemId: null,
                  disabled: true,
                  sourceOnly: true,
                  dragIdPrefix: activeDrag.dragIdPrefix,
                  isOverlay: true,
                  constraintsMapRef: { current: {} },
                  onResizeStart: () => {},
                  onResizeMove: () => {},
                  onResizeEnd: () => {},
                }}
              >
                <WidgetComponent id={activeDrag.itemId} />
              </DndGridProvider>
            </div>
          )
        })()}
    </DragOverlay>
  )
}

const WidgetRender = () => {
  const { isEditing, setOpenSheet } = useDashboardContext()

  const [layoutStorage, setLayoutStorage, { isLoading: layoutLoading }] =
    useKvStorage<LayoutStorage>('dashboard-widgets', DEFAULT_LAYOUTS)

  const [gridSize, setGridSize] = useState<GridSize | null>(null)

  // The layout saved for this grid size, or one adapted to it. The grid
  // renders no items until it has measured its size.
  const displayItems = useMemo(
    () => (gridSize ? layoutForSize(layoutStorage, gridSize) : []),
    [layoutStorage, gridSize],
  )

  const layoutStorageRef = useRef(layoutStorage)
  layoutStorageRef.current = layoutStorage

  const displayItemsRef = useRef(displayItems)
  displayItemsRef.current = displayItems

  const gridSizeRef = useRef<GridSize>({ cols: 1, rows: 1 })

  const handleSizeChange = useCallback((newSize: GridSize) => {
    gridSizeRef.current = newSize
    setGridSize(newSize)
  }, [])

  const handleLayoutChange = useCallback(
    (newItems: DashboardItem[]) => {
      const key = sizeKey(gridSizeRef.current)
      displayItemsRef.current = newItems

      layoutStorageRef.current = {
        ...layoutStorageRef.current,
        [key]: newItems,
      }
      setLayoutStorage(layoutStorageRef.current)
    },
    [setLayoutStorage],
  )

  const handleCloseClick = useCallback(
    (id: string) =>
      handleLayoutChange(displayItemsRef.current.filter((i) => i.id !== id)),
    [handleLayoutChange],
  )

  const handleGridLayoutChange = useCallback(
    (newItems: DndGridItemType<string>[]) =>
      handleLayoutChange(normalizeItems(newItems)),
    [handleLayoutChange],
  )

  const renderWidget = useCallback(
    (item: DndGridItemType<string>) => {
      const WidgetComponent = RENDER_MAP[(item as DashboardItem).type]

      return <WidgetComponent id={item.id} onCloseClick={handleCloseClick} />
    },
    [handleCloseClick],
  )

  const addWidgetFromSheet = useCallback(
    (widgetId: WidgetId) => {
      const { minW, minH } = WIDGET_MIN_SIZE_MAP[widgetId]
      const { cols, rows } = gridSizeRef.current
      const current = displayItemsRef.current
      const instanceId = crypto.randomUUID()

      const findPlacement = (): DashboardItem => {
        for (let y = 0; y <= rows; y++) {
          for (let x = 0; x <= cols - minW; x++) {
            const candidate: DashboardItem = {
              id: instanceId,
              type: widgetId,
              x,
              y,
              w: minW,
              h: minH,
            }
            if (!hasOverlap(current, instanceId, candidate)) {
              return candidate
            }
          }
        }
        const maxY = current.reduce((m, i) => Math.max(m, i.y + i.h), 0)
        return {
          id: instanceId,
          type: widgetId,
          x: 0,
          y: maxY,
          w: minW,
          h: minH,
        }
      }

      handleLayoutChange([...current, findPlacement()])
    },
    [handleLayoutChange],
  )

  return (
    <DndGridRoot>
      <div
        className="flex min-h-0 flex-1 flex-col p-4"
        data-slot="dashboard-widget-container"
      >
        <DndGrid
          gridId="main"
          className="min-h-0 flex-1"
          items={displayItems}
          onLayoutChange={handleGridLayoutChange}
          minCellSize={64}
          onSizeChange={handleSizeChange}
          gap={16}
          disabled={!isEditing || layoutLoading}
        >
          {renderWidget}
        </DndGrid>
      </div>

      <DashboardDragOverlay displayItems={displayItems} />

      <WidgetSheet
        onSourceDrop={(id) => addWidgetFromSheet(id)}
        onSourceDragStart={() => setOpenSheet(false)}
      />
    </DndGridRoot>
  )
}

function RouteComponent() {
  const { setIsEditing, setOpenSheet } = useDashboardContext()

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <div
          data-slot="dashboard-container"
          className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
        >
          <WidgetRender />

          <EditAction />
        </div>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onSelect={() => setIsEditing(true)}>
          <EditRounded className="size-4" />

          <span>{m.dashboard_context_menu_edit_widgets()}</span>
        </ContextMenuItem>

        <ContextMenuItem
          onSelect={() => {
            setIsEditing(true)
            setOpenSheet(true)
          }}
        >
          <AddRounded className="size-4" />

          <span>{m.dashboard_context_menu_add_widgets()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}

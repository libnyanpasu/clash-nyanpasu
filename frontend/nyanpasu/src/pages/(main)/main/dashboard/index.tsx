import AddRounded from '~icons/material-symbols/add-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { useCallback, useMemo, useRef, useState } from 'react'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  DndGrid,
  DndGridProvider,
  DndGridRoot,
  useDndGridRoot,
  type DndGridItemType,
  type GridPosition,
  type GridSize,
} from '@nyanpasu/ui/dnd-grid'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import {
  DashboardItem,
  DEFAULT_ITEMS,
  DEFAULT_LAYOUTS,
  LayoutStorage,
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WIDGET_RECOMMENDED_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts'
import { useDashboardContext } from '@/components/widgets/provider'
import { WidgetDataCalibration } from '@/components/widgets/widget-data-calibration'
import WidgetItem from '@/components/widgets/widget-item'
import { normalizeDashboardItems } from '@/components/widgets/widget-layout'
import { DashboardTrafficProvider } from '@/components/widgets/widget-traffic-provider'
import { m } from '@/paraglide/messages'
import { DragOverlay } from '@dnd-kit/core'
import { useKvStorage } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import EditAction from './_modules/edit-action'
import {
  adaptLayout,
  findBestLayout,
  findClosestStoredLayout,
  sizeKey,
} from './_modules/layout-adapt'
import { placeWidget } from './_modules/widget-placement'
import { WidgetSheet } from './_modules/widget-sheet'

export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
})

// Widgets declare these minimum sizes; they are known before any widget has
// rendered.
const constraintsFor = (items: DashboardItem[]) =>
  Object.fromEntries(
    items.map((item) => [item.id, WIDGET_MIN_SIZE_MAP[item.type]]),
  )

function layoutForSize(storage: LayoutStorage, size: GridSize) {
  const bestLayout = findBestLayout(storage, size)
  if (bestLayout) {
    return normalizeDashboardItems(bestLayout)
  }

  const base = normalizeDashboardItems(
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
  const { isEditing, setOpenSheet, configLoading, configReadError } =
    useDashboardContext()

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
      handleLayoutChange(normalizeDashboardItems(newItems)),
    [handleLayoutChange],
  )

  const renderWidget = useCallback(
    (item: DndGridItemType<string>) => {
      const type = (item as DashboardItem).type
      const WidgetComponent = RENDER_MAP[type]

      if (configLoading || configReadError) {
        return (
          <WidgetItem
            id={item.id}
            widgetType={type}
            {...WIDGET_MIN_SIZE_MAP[type]}
            onCloseClick={handleCloseClick}
          >
            <Card className="size-full">
              <CardContent
                className="size-full justify-center text-sm"
                role="status"
              >
                {configReadError
                  ? m.dashboard_widget_config_load_failed()
                  : m.dashboard_widget_config_loading()}
              </CardContent>
            </Card>
          </WidgetItem>
        )
      }

      return <WidgetComponent id={item.id} onCloseClick={handleCloseClick} />
    },
    [handleCloseClick, configLoading, configReadError],
  )

  const addWidgetFromSheet = useCallback(
    (widgetId: WidgetId, position?: GridPosition) => {
      const { cols, rows } = gridSizeRef.current
      const current = displayItemsRef.current
      const instanceId = crypto.randomUUID()
      const placement = placeWidget({
        id: instanceId,
        type: widgetId,
        items: current,
        cols,
        rows,
        minimum: WIDGET_MIN_SIZE_MAP[widgetId],
        recommended: WIDGET_RECOMMENDED_SIZE_MAP[widgetId],
        position,
      })

      if (placement) {
        handleLayoutChange([...current, placement])
      }
    },
    [handleLayoutChange],
  )

  return (
    <DndGridRoot>
      <div
        className="flex min-h-0 flex-1 flex-col p-4"
        data-slot="dashboard-widget-container"
      >
        <DashboardTrafficProvider items={displayItems}>
          <WidgetDataCalibration items={displayItems} />

          <DndGrid
            gridId="main"
            className="min-h-0 flex-1"
            items={displayItems}
            onLayoutChange={handleGridLayoutChange}
            onExternalDrop={(id, position) =>
              addWidgetFromSheet(id as WidgetId, position)
            }
            minCellSize={64}
            onSizeChange={handleSizeChange}
            gap={16}
            disabled={!isEditing || layoutLoading}
          >
            {renderWidget}
          </DndGrid>
        </DashboardTrafficProvider>
      </div>

      <DashboardDragOverlay displayItems={displayItems} />

      <WidgetSheet
        onAdd={addWidgetFromSheet}
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

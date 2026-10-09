import AddRounded from '~icons/material-symbols/add-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
  DEFAULT_LAYOUTS,
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
import { adaptLayout } from './_modules/layout-adapt'
import {
  normalizeLayoutStorage,
  saveLayout,
  selectLayoutEntry,
  type DashboardLayoutStorage,
} from './_modules/layout-storage'
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

  const [
    layoutStorage,
    setLayoutStorage,
    { isLoading: layoutLoading, readError: layoutReadError },
  ] = useKvStorage<DashboardLayoutStorage>(
    'dashboard-widgets',
    normalizeLayoutStorage(DEFAULT_LAYOUTS, DEFAULT_LAYOUTS),
    { migrate: (value) => normalizeLayoutStorage(value, DEFAULT_LAYOUTS) },
  )

  const [gridSize, setGridSize] = useState<GridSize | null>(null)
  const [activeLayoutId, setActiveLayoutId] = useState<string | null>(null)
  const gridSizeRef = useRef<GridSize | null>(null)

  const layoutStorageRef = useRef(layoutStorage)
  layoutStorageRef.current = layoutStorage

  const activeLayoutIdRef = useRef(activeLayoutId)
  activeLayoutIdRef.current = activeLayoutId

  useEffect(() => {
    const gridSize = gridSizeRef.current
    const nextId =
      layoutStorage.preferredId ??
      (gridSize ? selectLayoutEntry(layoutStorage, gridSize)?.id : null) ??
      null

    activeLayoutIdRef.current = nextId
    setActiveLayoutId(nextId)
  }, [layoutStorage])

  const sourceLayout = useMemo(() => {
    if (!gridSize) return null

    return (
      layoutStorage.entries.find(({ id }) => id === activeLayoutId) ??
      selectLayoutEntry(layoutStorage, gridSize)
    )
  }, [layoutStorage, activeLayoutId, gridSize])

  // Adapt from the pinned source each time; never persist this projection.
  const displayItems = useMemo(() => {
    if (!gridSize || !sourceLayout) return []

    const items = normalizeDashboardItems(sourceLayout.items)
    return adaptLayout(items, gridSize, constraintsFor(items))
  }, [gridSize, sourceLayout])

  const displayItemsRef = useRef(displayItems)
  displayItemsRef.current = displayItems

  const handleSizeChange = useCallback((newSize: GridSize) => {
    gridSizeRef.current = newSize
    setGridSize(newSize)

    if (activeLayoutIdRef.current === null) {
      const id =
        selectLayoutEntry(layoutStorageRef.current, newSize)?.id ?? null
      activeLayoutIdRef.current = id
      setActiveLayoutId(id)
    }
  }, [])

  const handleLayoutChange = useCallback(
    (newItems: DashboardItem[]) => {
      if (layoutLoading || layoutReadError) return

      const gridSize = gridSizeRef.current
      const source =
        layoutStorageRef.current.entries.find(
          ({ id }) => id === activeLayoutIdRef.current,
        ) ??
        (gridSize
          ? selectLayoutEntry(layoutStorageRef.current, gridSize)
          : null)
      if (!source) return

      const visibleIds = new Set(displayItemsRef.current.map(({ id }) => id))
      const hiddenSourceItems = source.items.filter(
        ({ id }) => !visibleIds.has(id),
      )
      displayItemsRef.current = newItems
      layoutStorageRef.current = saveLayout(
        layoutStorageRef.current,
        source.id,
        [...newItems, ...hiddenSourceItems],
      )
      activeLayoutIdRef.current = source.id
      setActiveLayoutId(source.id)
      setLayoutStorage(layoutStorageRef.current)
    },
    [layoutLoading, layoutReadError, setLayoutStorage],
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
      if (layoutLoading || layoutReadError) return

      const gridSize = gridSizeRef.current
      if (!gridSize) return

      const { cols, rows } = gridSize
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
    [handleLayoutChange, layoutLoading, layoutReadError],
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
            disabled={!isEditing || layoutLoading || Boolean(layoutReadError)}
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

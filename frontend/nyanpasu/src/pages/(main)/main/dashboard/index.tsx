import AddRounded from '~icons/material-symbols/add-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  DndGrid,
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
  WidgetId,
} from '@/components/widgets/consts'
import { WidgetGridHost, WidgetOverlayHost } from '@/components/widgets/core'
import { useDashboardContext } from '@/components/widgets/provider'
import { getWidgetDefinition } from '@/components/widgets/registry'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WidgetConfig,
} from '@/components/widgets/widget-config'
import { WidgetConfigControl } from '@/components/widgets/widget-config-control'
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

// The layout picked for the grid's size stays while the window is being
// resized, so widgets that a smaller grid hides come back as it grows again.
// Once the size has settled this long, the layout is picked again for the
// new size, as it is when the page opens.
const SETTLED_RESIZE_MS = 200

// Widgets declare these minimum sizes; they are known before any widget has
// rendered.
const constraintsFor = (items: DashboardItem[]) =>
  Object.fromEntries(
    items.map((item) => [item.id, getWidgetDefinition(item.type).size]),
  )

function DashboardDragOverlay({
  displayItems,
}: {
  displayItems: DashboardItem[]
}) {
  const root = useDndGridRoot()
  const activeDrag = root?.activeDrag ?? null
  const { configs } = useDashboardContext()

  if (!activeDrag) {
    return <DragOverlay dropAnimation={null} />
  }

  const found = displayItems.find((item) => item.id === activeDrag.itemId)
  const type = found?.type ?? (activeDrag.itemId as WidgetId)

  if (!Object.values(WidgetId).includes(type)) {
    return <DragOverlay dropAnimation={null} />
  }

  const definition = getWidgetDefinition(type)
  const config = getWidgetConfig(configs, activeDrag.itemId, type)
  const span = found
    ? { w: found.w, h: found.h }
    : { w: definition.size.defaultW, h: definition.size.defaultH }

  return (
    <DragOverlay dropAnimation={null}>
      <div
        className="cursor-grabbing rounded-2xl opacity-90"
        style={{
          width: activeDrag.dims.width,
          height: activeDrag.dims.height,
        }}
      >
        <WidgetOverlayHost
          type={type}
          definition={definition}
          config={config}
          span={span}
        />
      </div>
    </DragOverlay>
  )
}

const WidgetRender = () => {
  const {
    isEditing,
    setIsEditing,
    configs,
    saveConfig,
    configLoading,
    configReadError,
  } = useDashboardContext()

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

  const placedTypes = useMemo(
    () => new Set(displayItems.map(({ type }) => type)),
    [displayItems],
  )

  const selectForSize = useCallback((size: GridSize) => {
    const id = selectLayoutEntry(layoutStorageRef.current, size)?.id ?? null
    activeLayoutIdRef.current = id
    setActiveLayoutId(id)
  }, [])

  const settledTimerRef = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(settledTimerRef.current), [])

  const handleSizeChange = useCallback(
    (newSize: GridSize) => {
      gridSizeRef.current = newSize
      setGridSize(newSize)

      window.clearTimeout(settledTimerRef.current)
      if (activeLayoutIdRef.current === null) {
        selectForSize(newSize)
      } else {
        settledTimerRef.current = window.setTimeout(
          () => selectForSize(newSize),
          SETTLED_RESIZE_MS,
        )
      }
    },
    [selectForSize],
  )

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

  const canEdit = isEditing && !layoutLoading && !layoutReadError

  const renderWidget = useCallback(
    (item: DndGridItemType<string>) => {
      const dashboardItem = item as DashboardItem
      const definition = getWidgetDefinition(dashboardItem.type)

      if (configLoading || configReadError) {
        return (
          <WidgetItem
            id={item.id}
            widgetType={dashboardItem.type}
            minW={definition.size.minW}
            minH={definition.size.minH}
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

      const config = getWidgetConfig(
        configs,
        dashboardItem.id,
        dashboardItem.type,
      )

      return (
        <WidgetGridHost
          item={dashboardItem}
          config={config}
          configDisabled={configLoading || configReadError}
          interactive={!canEdit}
          isEditing={canEdit}
          updateConfig={(patch) =>
            saveConfig(dashboardItem.id, {
              ...config,
              ...patch,
            } as WidgetConfig)
          }
          resetConfig={() =>
            saveConfig(
              dashboardItem.id,
              DEFAULT_WIDGET_CONFIGS[dashboardItem.type],
            )
          }
          enterEditMode={() => setIsEditing(true)}
          onRemove={handleCloseClick}
        >
          <WidgetConfigControl
            id={dashboardItem.id}
            type={dashboardItem.type}
            isActive={canEdit}
          />
        </WidgetGridHost>
      )
    },
    [
      canEdit,
      configLoading,
      configReadError,
      configs,
      handleCloseClick,
      saveConfig,
      setIsEditing,
    ],
  )

  const addWidgetFromSheet = useCallback(
    (widgetId: WidgetId, position?: GridPosition) => {
      if (layoutLoading || layoutReadError) return

      const gridSize = gridSizeRef.current
      if (!gridSize) return

      const { cols, rows } = gridSize
      const current = displayItemsRef.current
      const instanceId = crypto.randomUUID()
      const definition = getWidgetDefinition(widgetId)
      const placement = placeWidget({
        id: instanceId,
        type: widgetId,
        items: current,
        cols,
        rows,
        minimum: definition.size,
        recommended: {
          w: definition.size.defaultW,
          h: definition.size.defaultH,
        },
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

      <WidgetSheet placedTypes={placedTypes} onAdd={addWidgetFromSheet} />
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

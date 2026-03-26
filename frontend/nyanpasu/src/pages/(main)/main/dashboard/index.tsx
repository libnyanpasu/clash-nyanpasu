import AddRounded from '~icons/material-symbols/add-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { useCallback, useRef, useState } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { ContextMenuItem } from '@/components/ui/context-menu'
import {
  DndGrid,
  DndGridProvider,
  DndGridRoot,
  useDndGridRoot,
  type DndGridItemType,
  type GridItemConstraints,
  type GridSize,
} from '@/components/ui/dnd-grid'
import { hasOverlap } from '@/components/ui/dnd-grid/utils'
import { m } from '@/paraglide/messages'
import { DragOverlay } from '@dnd-kit/core'
import { useKvStorage } from '@nyanpasu/interface'
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
                  displayItems: [],
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
                  dragIdPrefix: '',
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

  const [layoutStorage, setLayoutStorage] = useKvStorage<LayoutStorage>(
    'dashboard-widgets',
    DEFAULT_LAYOUTS,
  )

  const [displayItems, setDisplayItems] =
    useState<DashboardItem[]>(DEFAULT_ITEMS)

  const layoutStorageRef = useRef(layoutStorage)
  layoutStorageRef.current = layoutStorage

  const displayItemsRef = useRef(displayItems)
  displayItemsRef.current = displayItems

  const gridSizeRef = useRef<GridSize>({ cols: 1, rows: 1 })

  const handleSizeChange = useCallback(
    (
      newSize: GridSize,
      constraintsMap: Record<string, GridItemConstraints>,
    ) => {
      gridSizeRef.current = newSize

      const bestLayout = findBestLayout(layoutStorageRef.current, newSize)
      if (bestLayout) {
        const normalized = normalizeItems(bestLayout)
        displayItemsRef.current = normalized
        setDisplayItems(normalized)
        return
      }

      const base =
        findClosestStoredLayout(layoutStorageRef.current, newSize) ??
        DEFAULT_ITEMS

      const nextItems = normalizeItems(
        adaptLayout(base, newSize, constraintsMap),
      )
      displayItemsRef.current = nextItems
      setDisplayItems(nextItems)
    },
    [],
  )

  const handleLayoutChange = useCallback(
    (newItems: DashboardItem[]) => {
      const key = sizeKey(gridSizeRef.current)
      displayItemsRef.current = newItems
      setDisplayItems(newItems)

      layoutStorageRef.current = {
        ...layoutStorageRef.current,
        [key]: newItems,
      }
      setLayoutStorage(layoutStorageRef.current)
    },
    [setLayoutStorage],
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
      <div className="size-full p-4" data-slot="dashboard-widget-container">
        <DndGrid
          gridId="main"
          className="size-full"
          items={displayItems}
          onLayoutChange={(newItems) =>
            handleLayoutChange(normalizeItems(newItems))
          }
          minCellSize={64}
          onSizeChange={handleSizeChange}
          gap={16}
          disabled={!isEditing}
        >
          {(item) => {
            const WidgetComponent = RENDER_MAP[(item as DashboardItem).type]

            return (
              <WidgetComponent
                id={item.id}
                onCloseClick={() =>
                  handleLayoutChange(
                    displayItemsRef.current.filter((i) => i.id !== item.id),
                  )
                }
              />
            )
          }}
        </DndGrid>
      </div>

      <DashboardDragOverlay displayItems={displayItemsRef.current} />

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
          className="relative size-full overflow-hidden"
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

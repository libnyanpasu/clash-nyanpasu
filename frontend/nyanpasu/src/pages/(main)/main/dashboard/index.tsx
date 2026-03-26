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
  type DndGridItemType,
  type GridItemConstraints,
  type GridSize,
} from '@/components/ui/dnd-grid'
import { m } from '@/paraglide/messages'
import { useKvStorage } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { useLocalStorage } from '@uidotdev/usehooks'
import {
  DEFAULT_ITEMS,
  DEFAULT_LAYOUTS,
  LayoutStorage,
  RENDER_MAP,
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

export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
})

const WidgetRender = () => {
  const { isEditing } = useDashboardContext()

  // Only layouts explicitly saved by the user (via onLayoutChange) are persisted.
  const [layoutStorage, setLayoutStorage] = useKvStorage<LayoutStorage>(
    'dashboard-widgets',
    DEFAULT_LAYOUTS,
  )

  // Transient display items — may be auto-adapted and are NOT written to storage.
  const [displayItems, setDisplayItems] =
    useState<DndGridItemType<WidgetId>[]>(DEFAULT_ITEMS)

  // Refs for use inside callbacks without stale-closure issues.
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

      // 1. A stored layout that fits within the new size — use directly.
      const bestLayout = findBestLayout(layoutStorageRef.current, newSize)
      if (bestLayout) {
        displayItemsRef.current = bestLayout
        setDisplayItems(bestLayout)
        return
      }

      // 2. No fitting layout — adapt from the closest stored layout so the result
      //    is always derived from something the user explicitly saved.
      //    Fall back to DEFAULT_ITEMS only if storage is completely empty.
      const base =
        findClosestStoredLayout(layoutStorageRef.current, newSize) ??
        DEFAULT_ITEMS

      const nextItems = adaptLayout(base, newSize, constraintsMap)
      displayItemsRef.current = nextItems
      setDisplayItems(nextItems)
    },
    [],
  )

  const handleLayoutChange = useCallback(
    (newItems: DndGridItemType<WidgetId>[]) => {
      const key = sizeKey(gridSizeRef.current)
      displayItemsRef.current = newItems
      setDisplayItems(newItems)

      // Update ref immediately so any rapid size change after saving sees the
      // latest storage without waiting for a React re-render to commit.
      layoutStorageRef.current = {
        ...layoutStorageRef.current,
        [key]: newItems,
      }
      setLayoutStorage(layoutStorageRef.current)
    },
    [setLayoutStorage],
  )

  return (
    <div className="size-full p-4" data-slot="dashboard-widget-container">
      <DndGrid
        className="size-full"
        items={displayItems}
        onLayoutChange={handleLayoutChange}
        minCellSize={64}
        onSizeChange={handleSizeChange}
        gap={16}
        disabled={!isEditing}
      >
        {(item) => {
          const WidgetComponent = RENDER_MAP[item.id]

          return <WidgetComponent />
        }}
      </DndGrid>
    </div>
  )
}

function RouteComponent() {
  const { setIsEditing } = useDashboardContext()

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
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}

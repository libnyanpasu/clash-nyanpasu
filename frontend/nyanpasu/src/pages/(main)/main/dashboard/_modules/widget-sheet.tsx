import AddRounded from '~icons/material-symbols/add-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { useMemo, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { DndGrid, GridSize, useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerTitle,
} from '@nyanpasu/ui/drawer'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts'
import { useDashboardContext } from '@/components/widgets/provider'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

function SheetWidget({
  id,
  onAdd,
}: {
  id: WidgetId
  onAdd: (id: WidgetId) => void
}) {
  const { displayItems, getItemRect, isOverlay } = useDndGridContext()
  const item = displayItems.find((entry) => entry.id === id)
  const rect = item ? getItemRect(item) : null
  const WidgetComponent = RENDER_MAP[id]
  return (
    <>
      <WidgetComponent id={id} />
      {rect && !isOverlay && (
        <Button
          variant="raised"
          icon
          className="absolute z-10 size-7"
          style={{ left: rect.left + rect.width - 32, top: rect.top + 4 }}
          aria-label={m.dashboard_add_widget()}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => onAdd(id)}
        >
          <AddRounded className="size-4" />
        </Button>
      )}
    </>
  )
}

export function WidgetSheet({
  onSourceDrop,
  onSourceDragStart,
}: {
  onSourceDrop: (id: WidgetId) => void
  onSourceDragStart: () => void
}) {
  const { openSheet, setOpenSheet } = useDashboardContext()

  const [gridSize, setGridSize] = useState<GridSize>()

  const sheetItems = useMemo(() => {
    if (!gridSize) {
      return []
    }

    const ids = Object.keys(RENDER_MAP) as WidgetId[]
    const result = []
    let rowX = 0
    let rowY = 0
    let rowH = 0

    for (const id of ids) {
      const { minW: w, minH: h } = WIDGET_MIN_SIZE_MAP[id]
      if (rowX + w > gridSize.cols) {
        rowY += rowH
        rowX = 0
        rowH = 0
      }
      result.push({ id, x: rowX, y: rowY, w, h })
      rowX += w
      rowH = Math.max(rowH, h)
    }

    return result
  }, [gridSize])

  const contentRows = sheetItems.reduce(
    (rows, item) => Math.max(rows, item.y + item.h),
    1,
  )

  return (
    <Drawer open={openSheet} onOpenChange={setOpenSheet}>
      <DrawerContent
        className="h-full max-h-1/2 min-h-96 max-w-96"
        aria-describedby={undefined}
      >
        <div className="flex items-center justify-between gap-4 p-4">
          <DrawerTitle className="text-lg font-semibold">
            {m.dashboard_add_widget()}
          </DrawerTitle>

          <DrawerClose asChild>
            <Button
              variant="raised"
              className="size-8"
              icon
              aria-label={m.dashboard_widget_config_close_library()}
            >
              <CloseRounded className="size-4" />
            </Button>
          </DrawerClose>
        </div>

        <ScrollArea
          className={cn(
            'min-h-0 flex-1',
            '[&_[data-slot=scroll-area-viewport]>div]:block!',
            '[&_[data-slot=scroll-area-viewport]>div]:h-full',
          )}
        >
          <div
            className="flex w-full flex-col px-4"
            style={{ height: Math.max(384, contentRows * 80 - 16) }}
          >
            <DndGrid
              gridId="sheet"
              className="min-h-0 flex-1"
              items={sheetItems}
              minCellSize={64}
              gap={16}
              size={
                gridSize
                  ? { cols: gridSize.cols, rows: contentRows }
                  : undefined
              }
              disabled={false}
              sourceOnly
              dragIdPrefix="sheet:"
              onSourceDrop={(id) => onSourceDrop(id as WidgetId)}
              onSourceDragStart={onSourceDragStart}
              onSizeChange={(size) => setGridSize(size)}
            >
              {(item) => {
                return (
                  <SheetWidget id={item.id as WidgetId} onAdd={onSourceDrop} />
                )
              }}
            </DndGrid>
          </div>
        </ScrollArea>
      </DrawerContent>
    </Drawer>
  )
}

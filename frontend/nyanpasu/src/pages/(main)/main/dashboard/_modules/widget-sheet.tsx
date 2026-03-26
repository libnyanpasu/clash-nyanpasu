import CloseRounded from '~icons/material-symbols/close-rounded'
import { useMemo, useState } from 'react'
import { Drawer } from 'vaul'
import { Button } from '@/components/ui/button'
import { DndGrid, GridSize } from '@/components/ui/dnd-grid'
import { ScrollArea } from '@/components/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
import { RENDER_MAP, WIDGET_MIN_SIZE_MAP, WidgetId } from './consts'
import { useDashboardContext } from './provider'

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

  return (
    <Drawer.Root open={openSheet} onOpenChange={setOpenSheet}>
      <Drawer.Portal>
        <Drawer.Overlay className="fixed inset-0 bg-black/30" />

        <Drawer.Content
          className={cn(
            'fixed inset-x-0 bottom-0 z-50 mx-auto max-w-96 min-w-96',
            'dark:bg-surface/30 bg-surface-variant/30 backdrop-blur-3xl',
            'h-full max-h-1/2 min-h-96 rounded-t-2xl',
            'dark:border-surface-variant/50 border-surface/50 border',
            'flex flex-col',
          )}
          aria-describedby={undefined}
        >
          <div className="flex items-center justify-between gap-4 p-4">
            <Drawer.Title className="text-lg font-semibold">
              {m.dashboard_add_widget()}
            </Drawer.Title>

            <Drawer.Close asChild>
              <Button variant="raised" className="size-8" icon>
                <CloseRounded className="size-4" />
              </Button>
            </Drawer.Close>
          </div>

          <ScrollArea
            className={cn(
              'min-h-0 flex-1',
              '[&_[data-slot=scroll-area-viewport]>div]:block!',
              '[&_[data-slot=scroll-area-viewport]>div]:h-full',
            )}
          >
            <div className="flex h-full w-full flex-col px-4">
              <DndGrid
                gridId="sheet"
                className="min-h-0 flex-1"
                items={sheetItems}
                minCellSize={64}
                gap={16}
                disabled={false}
                sourceOnly
                dragIdPrefix="sheet:"
                onSourceDrop={(id) => onSourceDrop(id as WidgetId)}
                onSourceDragStart={onSourceDragStart}
                onSizeChange={(size) => setGridSize(size)}
              >
                {(item) => {
                  const WidgetComponent = RENDER_MAP[item.id as WidgetId]

                  return (
                    <WidgetComponent id={item.id} onCloseClick={() => {}} />
                  )
                }}
              </DndGrid>
            </div>
          </ScrollArea>
        </Drawer.Content>
      </Drawer.Portal>
    </Drawer.Root>
  )
}

import DragIndicatorRounded from '~icons/material-symbols/drag-indicator-rounded'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { cn } from '@nyanpasu/utils'
import type {
  ColumnOrderState,
  ColumnVisibilityState,
} from '@tanstack/react-table'

function ColumnItem({
  id,
  index,
  label,
  visible,
  locked,
  onVisibleChange,
}: {
  id: string
  index: number
  label: string
  visible: boolean
  locked: boolean
  onVisibleChange: (visible: boolean) => void
}) {
  const { ref, handleRef, isDragging } = useSortable({ id, index })

  return (
    <div
      ref={ref}
      className={cn(
        'flex h-12 items-center gap-2 rounded-xl pr-3 pl-1',
        'bg-surface-variant/30 dark:bg-surface-variant/10',
        isDragging && 'opacity-60',
      )}
      data-slot="connections-column-settings-item"
    >
      <button
        ref={handleRef}
        type="button"
        className={cn(
          'text-on-surface-variant grid size-9 shrink-0 cursor-grab touch-none place-content-center',
          'focus-visible:ring-primary rounded-lg outline-none focus-visible:ring-2',
        )}
        aria-label={m.connections_column_settings_reorder()}
      >
        <DragIndicatorRounded className="size-5" />
      </button>

      <span className="min-w-0 flex-1 truncate text-sm">{label}</span>

      <Switch
        checked={visible}
        disabled={locked}
        onCheckedChange={onVisibleChange}
      />
    </div>
  )
}

export default function ColumnSettingsModal({
  open,
  onOpenChange,
  labels,
  order,
  visibility,
  onOrderChange,
  onVisibilityChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  labels: ReadonlyMap<string, () => string>
  order: ColumnOrderState
  visibility: ColumnVisibilityState
  onOrderChange: (order: ColumnOrderState) => void
  onVisibilityChange: (visibility: ColumnVisibilityState) => void
}) {
  const visibleCount = order.filter((id) => visibility[id] !== false).length

  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      {/* Focusing the first drag handle on open would mark it as if picked. */}
      <ModalContent onOpenAutoFocus={(event) => event.preventDefault()}>
        <Card divider className="flex w-96 max-w-[90vw] flex-col">
          <CardHeader>
            <ModalTitle>{m.connections_column_settings()}</ModalTitle>
          </CardHeader>

          <CardContent asChild className="p-0">
            <ScrollArea className="max-h-[60vh]">
              <DragDropProvider
                onDragEnd={(event) => {
                  const next = move(order, event)

                  // A cancelled drag or a drop in place changes nothing.
                  if (next.some((id, index) => id !== order[index])) {
                    onOrderChange(next)
                  }
                }}
              >
                <div className="flex flex-col gap-2 p-4">
                  {order.map((id, index) => {
                    const visible = visibility[id] !== false

                    return (
                      <ColumnItem
                        key={id}
                        id={id}
                        index={index}
                        label={labels.get(id)?.() ?? id}
                        visible={visible}
                        // The table needs at least one column to render.
                        locked={visible && visibleCount === 1}
                        onVisibleChange={(value) =>
                          onVisibilityChange({ ...visibility, [id]: value })
                        }
                      />
                    )
                  })}
                </div>
              </DragDropProvider>
            </ScrollArea>
          </CardContent>

          <CardFooter>
            <ModalClose variant="flat">{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

import AddRounded from '~icons/material-symbols/add-rounded'
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { cardVariants } from '@nyanpasu/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { Switch } from '@nyanpasu/ui/switch'
import useCustomCss, { reportCustomCssError } from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import type { CustomCssItem } from '@/utils/custom-css-items'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { useLockFn } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import {
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import CustomCssNameDialog from './custom-css-name-dialog'

const LARGE_CSS_CHARS = 100_000

/** Run a change and tell the user if it was not saved. */
async function attempt(change: () => Promise<unknown>) {
  try {
    await change()
  } catch (error) {
    await reportCustomCssError(error)
  }
}

function CustomCssListItem({
  item,
  index,
  count,
  onEdit,
  onRename,
  onToggle,
  onMove,
  onRemove,
}: {
  item: CustomCssItem
  index: number
  count: number
  onEdit: () => void
  onRename: () => void
  onToggle: (enabled: boolean) => void
  onMove: (offset: number) => void
  onRemove: () => void
}) {
  const { ref, isDragging, isDropTarget } = useSortable({
    id: item.id,
    index,
  })

  const charCount = item.source.length

  return (
    <div
      ref={ref}
      tabIndex={0}
      aria-label={m.settings_user_interface_custom_css_reorder({
        name: item.name,
      })}
      className={cn(
        cardVariants(),
        'flex min-w-0 items-center gap-4 px-5 py-4 select-none',
        // Only colours transition: dnd-kit animates the item's position itself.
        'transition-colors motion-reduce:transition-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
        'hover:bg-surface-variant/60 cursor-grab touch-none active:cursor-grabbing',
        isDragging && 'bg-secondary-container shadow-lg',
        isDropTarget && 'ring-primary ring-2',
      )}
      data-slot="custom-css-item"
      data-item-id={item.id}
      data-dragging={isDragging || undefined}
    >
      <div className="min-w-0 flex-1" data-slot="custom-css-item-label">
        <p className="text-base font-medium [overflow-wrap:anywhere]">
          {item.name}
        </p>

        <p className="text-on-surface-variant text-sm">
          {charCount > 0
            ? m.settings_user_interface_custom_css_chars({ count: charCount })
            : m.settings_user_interface_custom_css_empty()}

          {charCount > LARGE_CSS_CHARS && (
            <span className="text-warning ml-2">
              {m.settings_user_interface_custom_css_large_warning()}
            </span>
          )}
        </p>
      </div>

      <Button
        type="button"
        icon
        aria-label={m.settings_user_interface_custom_css_edit()}
        className="text-on-surface-variant size-9 shrink-0"
        data-slot="custom-css-item-edit"
        onClick={onEdit}
      >
        <EditRounded className="size-5" aria-hidden />
      </Button>

      <Switch
        aria-label={item.name}
        checked={item.enabled}
        onCheckedChange={onToggle}
      />

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            icon
            aria-label={m.settings_user_interface_custom_css_row_actions({
              name: item.name,
            })}
            className="text-on-surface-variant size-9 shrink-0"
            data-slot="custom-css-item-actions"
          >
            <MoreVertRounded className="size-5" aria-hidden />
          </Button>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={onRename}>
            <EditRounded className="size-5" aria-hidden />
            {m.settings_user_interface_custom_css_rename()}
          </DropdownMenuItem>

          <DropdownMenuItem disabled={index === 0} onSelect={() => onMove(-1)}>
            <ArrowUpwardRounded className="size-5" aria-hidden />
            {m.settings_user_interface_custom_css_move_up()}
          </DropdownMenuItem>

          <DropdownMenuItem
            disabled={index === count - 1}
            onSelect={() => onMove(1)}
          >
            <ArrowDownwardRounded className="size-5" aria-hidden />
            {m.settings_user_interface_custom_css_move_down()}
          </DropdownMenuItem>

          <DropdownMenuSeparator />

          <DropdownMenuItem className="text-error" onSelect={onRemove}>
            <DeleteOutlineRounded className="size-5" aria-hidden />
            {m.settings_user_interface_custom_css_delete()}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

/**
 * The user's own CSS items, in cascade order: later items win. Any number can
 * be enabled; dragging an item or the row menu changes its place.
 */
export default function CustomCssItems() {
  const customCss = useCustomCss()

  const { items } = customCss.state

  const [dialog, setDialog] = useState<
    { kind: 'add' } | { kind: 'rename'; item: CustomCssItem } | null
  >(null)

  const openEditor = useLockFn(async (id: string) => {
    await rpc.createEditorWindow('css-editor', id)
  })

  const moveItem = (index: number, offset: number) => {
    const ids = items.map((item) => item.id)
    const [id] = ids.splice(index, 1)

    ids.splice(index + offset, 0, id)
    attempt(() => customCss.reorder(ids))
  }

  return (
    <section className="flex flex-col" data-slot="custom-css-items">
      <div
        className="flex items-center justify-between gap-3 pr-1"
        data-slot="custom-css-items-header"
      >
        <SettingsLabel>
          {m.settings_user_interface_custom_css_group()}
        </SettingsLabel>

        <Button
          variant="flat"
          className="flex items-center gap-2"
          data-slot="custom-css-add"
          onClick={() => setDialog({ kind: 'add' })}
        >
          <AddRounded className="size-5" aria-hidden />

          <span>{m.settings_user_interface_custom_css_add()}</span>
        </Button>
      </div>

      {items.length === 0 && (
        <div
          className={cn(cardVariants(), 'p-3')}
          data-slot="custom-css-items-empty"
        >
          <EmptyState
            size="compact"
            tone="normal"
            message={m.settings_user_interface_custom_css_empty()}
          />
        </div>
      )}

      <DragDropProvider
        sensors={WHOLE_ITEM_DRAG_SENSORS}
        onDragEnd={(event) => {
          if (event.canceled) {
            return
          }

          const ids = items.map((item) => item.id)
          const next = move(ids, event)

          if (next.some((id, index) => id !== ids[index])) {
            attempt(() => customCss.reorder(next))
          }
        }}
      >
        <SettingsGroup data-slot="custom-css-item-list">
          {items.map((item, index) => (
            <CustomCssListItem
              key={item.id}
              item={item}
              index={index}
              count={items.length}
              onEdit={() => openEditor(item.id)}
              onRename={() => setDialog({ kind: 'rename', item })}
              onToggle={(enabled) =>
                attempt(() => customCss.setEnabled(item.id, enabled))
              }
              onMove={(offset) => moveItem(index, offset)}
              onRemove={() => attempt(() => customCss.remove(item.id))}
            />
          ))}
        </SettingsGroup>
      </DragDropProvider>

      {dialog?.kind === 'add' && (
        <CustomCssNameDialog
          title={m.settings_user_interface_custom_css_add_title()}
          initialName={m.settings_user_interface_custom_css_new_item_name({
            number: items.length + 1,
          })}
          onSubmit={(name) =>
            attempt(async () => openEditor(await customCss.add(name)))
          }
          onClose={() => setDialog(null)}
        />
      )}

      {dialog?.kind === 'rename' && (
        <CustomCssNameDialog
          title={m.settings_user_interface_custom_css_rename_title()}
          initialName={dialog.item.name}
          onSubmit={(name) =>
            attempt(() => customCss.rename(dialog.item.id, name))
          }
          onClose={() => setDialog(null)}
        />
      )}
    </section>
  )
}

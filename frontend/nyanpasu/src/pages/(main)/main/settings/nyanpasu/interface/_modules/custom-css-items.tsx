import AddRounded from '~icons/material-symbols/add-rounded'
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Modal, ModalAnchor, ModalTrigger, useModal } from '@nyanpasu/ui/modal'
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
import { SettingsLabel } from '../../../../_modules/settings-card'
import {
  SettingsList,
  SettingsListActions,
  SettingsListContent,
  SettingsListItem,
} from '../../../../_modules/settings-list'
import {
  sortableItemVariants,
  useSortableTransition,
} from '../../../../_modules/sortable-item'
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
  const { ref, isDragging } = useSortable({
    id: item.id,
    index,
    transition: useSortableTransition(),
  })

  const charCount = item.source.length

  return (
    <SettingsListItem
      ref={ref}
      tabIndex={0}
      aria-label={m.settings_user_interface_custom_css_reorder({
        name: item.name,
      })}
      className={sortableItemVariants({
        draggable: true,
        dragging: isDragging,
      })}
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
        <ModalAnchor className="w-fit">
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
        </ModalAnchor>

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
    </SettingsListItem>
  )
}

/**
 * The user's own CSS items, in cascade order: later items win. Any number can
 * be enabled; dragging an item or the row menu changes its place.
 */
export default function CustomCssItems() {
  return (
    <Modal>
      <CustomCssItemsContent />
    </Modal>
  )
}

function CustomCssItemsContent() {
  const { setOpen } = useModal()
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
    <SettingsList data-slot="custom-css-items">
      <div data-slot="custom-css-items-header">
        <SettingsLabel>
          {m.settings_user_interface_custom_css_group()}
        </SettingsLabel>
      </div>

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
        <SettingsListContent
          data-slot="custom-css-item-list"
          empty={items.length === 0}
          emptyMessage={m.settings_user_interface_custom_css_empty()}
          description={
            <p>{m.settings_user_interface_custom_css_description()}</p>
          }
        >
          {items.map((item, index) => (
            <CustomCssListItem
              key={item.id}
              item={item}
              index={index}
              count={items.length}
              onEdit={() => openEditor(item.id)}
              onRename={() => {
                setDialog({ kind: 'rename', item })
                setOpen(true)
              }}
              onToggle={(enabled) =>
                attempt(() => customCss.setEnabled(item.id, enabled))
              }
              onMove={(offset) => moveItem(index, offset)}
              onRemove={() => attempt(() => customCss.remove(item.id))}
            />
          ))}
        </SettingsListContent>
      </DragDropProvider>

      <SettingsListActions>
        <ModalTrigger asChild>
          <Button
            variant="raised"
            className="flex items-center gap-1 px-4"
            data-slot="custom-css-add"
            onClick={() => setDialog({ kind: 'add' })}
          >
            <AddRounded className="size-6" aria-hidden />
            <span>{m.settings_user_interface_custom_css_add()}</span>
          </Button>
        </ModalTrigger>
      </SettingsListActions>

      <CustomCssNameDialog
        title={
          dialog?.kind === 'rename'
            ? m.settings_user_interface_custom_css_rename_title()
            : m.settings_user_interface_custom_css_add_title()
        }
        initialName={
          dialog?.kind === 'rename'
            ? dialog.item.name
            : m.settings_user_interface_custom_css_new_item_name({
                number: items.length + 1,
              })
        }
        onSubmit={async (name) => {
          if (dialog?.kind === 'rename') {
            await customCss.rename(dialog.item.id, name)
          } else {
            const id = await customCss.add(name)
            // Saving succeeded even if the separate editor window cannot open.
            await openEditor(id).catch(reportCustomCssError)
          }
        }}
      />
    </SettingsList>
  )
}

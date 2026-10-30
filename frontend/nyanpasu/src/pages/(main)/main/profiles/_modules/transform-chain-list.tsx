import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import FormatListNumberedRounded from '~icons/material-symbols/format-list-numbered-rounded'
import ManageSearchRounded from '~icons/material-symbols/manage-search-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { AnimatePresence } from 'motion/react'
import { memo, type ReactNode } from 'react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
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
import { LinearProgress } from '@nyanpasu/ui/progress'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import { SettingsGroup } from '../../_modules/settings-card'
import { RowAction } from './compact-controls'
import { PROFILE_TYPE_NAMES } from './consts'
import {
  StepGroup,
  StepRowIcon,
  StepRowText,
  stepRowVariants,
} from './step-list'
import { TransformAddMenu } from './transform-add-menu'
import {
  useTransformChainDraft,
  type TransformChainDraft,
} from './use-transform-chain-draft'
import { profileTypeOf, type TransformProfile } from './utils'

/**
 * `card` is a roomy card with a menu; `step` is a row of a grouped step list
 * with its actions in the open, which the whole row drags.
 */
export type TransformChainVariant = 'card' | 'step'

type ItemProps = {
  variant: TransformChainVariant
  profile: TransformProfile
  index: number
  count: number
  /** The Inspect step key of this row, matching the runtime node. */
  traceStep: string
  saving: boolean
  actionsDisabled: boolean
  onMove: (uid: string, offset: number) => void
  onRemove: (uid: string) => void
}

function CardItemContent({
  profile,
  index,
  count,
  traceStep,
  saving,
  actionsDisabled,
  onMove,
  onRemove,
  itemRef,
  dragging,
  dropTarget,
  draggable,
}: Omit<ItemProps, 'variant'> & {
  itemRef?: (element: Element | null) => void
  dragging?: boolean
  dropTarget?: boolean
  draggable?: boolean
}) {
  return (
    <div
      ref={itemRef}
      data-slot="profile-chain-editor-item"
      data-uid={profile.uid}
      data-dragging={dragging || undefined}
      aria-label={
        draggable
          ? m.profile_chain_editor_drag_item({ name: profile.name })
          : undefined
      }
      className={cn(
        cardVariants(),
        'flex min-w-0 items-center gap-3 py-2.5 pr-2.5 pl-3 select-none',
        // Only colours transition: dnd-kit animates the item's position itself.
        'transition-colors motion-reduce:transition-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
        draggable &&
          'hover:bg-surface-variant/60 cursor-grab touch-none active:cursor-grabbing',
        dragging && 'bg-secondary-container shadow-lg',
        dropTarget && 'ring-primary ring-2',
        saving && 'opacity-50',
      )}
    >
      <span
        data-slot="profile-chain-editor-order"
        className="bg-primary text-on-primary grid size-6 shrink-0 place-items-center rounded-full text-xs font-medium tabular-nums"
      >
        {index + 1}
      </span>

      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium [overflow-wrap:anywhere]">
          {profile.name}
        </p>

        <span
          data-slot="profile-chain-editor-type"
          className="bg-secondary-container/60 text-on-secondary-container mt-1 inline-block rounded-md px-1.5 py-0.5 text-xs"
        >
          {PROFILE_TYPE_NAMES[profileTypeOf(profile)]}
        </span>
      </div>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            icon
            disabled={actionsDisabled}
            data-slot="profile-chain-editor-actions"
            aria-label={m.profile_chain_editor_row_actions({
              name: profile.name,
            })}
            className="text-on-surface-variant size-9 shrink-0"
          >
            <MoreVertRounded className="size-5" aria-hidden />
          </Button>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end">
          <DropdownMenuItem
            onSelect={async () => {
              await rpc.createEditorWindow('profile', profile.uid)
            }}
          >
            {m.profile_chain_editor_open_editor()}
            <EditSquareOutlineRounded className="size-5" aria-hidden />
          </DropdownMenuItem>

          <DropdownMenuItem asChild>
            <Link to="/main/profiles/inspect" search={{ step: traceStep }}>
              {m.profile_chain_editor_trace()}
              <ManageSearchRounded className="size-5" aria-hidden />
            </Link>
          </DropdownMenuItem>

          <DropdownMenuSeparator />

          <DropdownMenuItem
            disabled={index === 0}
            onSelect={() => onMove(profile.uid, -1)}
          >
            {m.profile_chain_editor_move_up()}
            <ArrowUpwardRounded className="size-5" aria-hidden />
          </DropdownMenuItem>

          <DropdownMenuItem
            disabled={index === count - 1}
            onSelect={() => onMove(profile.uid, 1)}
          >
            {m.profile_chain_editor_move_down()}
            <ArrowDownwardRounded className="size-5" aria-hidden />
          </DropdownMenuItem>

          <DropdownMenuSeparator />

          <DropdownMenuItem
            className="text-error"
            onSelect={() => onRemove(profile.uid)}
          >
            {m.profile_chain_editor_remove()}
            <DeleteOutlineRounded className="size-5" aria-hidden />
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

function StepItemContent({
  profile,
  index,
  traceStep,
  saving,
  actionsDisabled,
  onRemove,
  itemRef,
  dragging,
  dropTarget,
  draggable,
}: Omit<ItemProps, 'variant'> & {
  itemRef?: (element: Element | null) => void
  dragging?: boolean
  dropTarget?: boolean
  draggable?: boolean
}) {
  return (
    <div
      ref={itemRef}
      data-slot="profile-chain-editor-item"
      data-uid={profile.uid}
      data-dragging={dragging || undefined}
      aria-label={
        draggable
          ? m.profile_chain_editor_drag_item({ name: profile.name })
          : undefined
      }
      className={cn(
        stepRowVariants({ interactive: draggable }),
        'py-1 select-none',
        // Only colours transition: dnd-kit animates the item's position itself.
        draggable && 'cursor-grab touch-none active:cursor-grabbing',
        dragging && 'bg-secondary-container shadow-lg',
        dropTarget && 'ring-primary ring-2',
        saving && 'opacity-50',
      )}
    >
      <span
        data-slot="profile-chain-editor-order"
        className="bg-secondary-container text-on-secondary-container grid size-6 shrink-0 place-items-center rounded-full text-xs font-semibold tabular-nums"
      >
        {index + 1}
      </span>

      <StepRowText
        title={profile.name}
        sub={PROFILE_TYPE_NAMES[profileTypeOf(profile)]}
      />

      <RowAction
        label={m.profile_chain_editor_open_editor()}
        data-slot="profile-chain-editor-open-editor"
        onClick={async () => {
          await rpc.createEditorWindow('profile', profile.uid)
        }}
      >
        <EditSquareOutlineRounded className="size-5" aria-hidden />
      </RowAction>

      <RowAction
        asChild
        label={m.profile_chain_editor_trace()}
        data-slot="profile-chain-editor-trace"
      >
        <Link to="/main/profiles/inspect" search={{ step: traceStep }}>
          <ManageSearchRounded className="size-5" aria-hidden />
        </Link>
      </RowAction>

      <RowAction
        label={m.profile_chain_editor_remove()}
        disabled={actionsDisabled}
        data-slot="profile-chain-editor-remove"
        onClick={() => onRemove(profile.uid)}
      >
        <DeleteOutlineRounded className="size-5" aria-hidden />
      </RowAction>
    </div>
  )
}

function ItemContent({
  variant,
  ...props
}: ItemProps & {
  itemRef?: (element: Element | null) => void
  dragging?: boolean
  dropTarget?: boolean
  draggable?: boolean
}) {
  return variant === 'step' ? (
    <StepItemContent {...props} />
  ) : (
    <CardItemContent {...props} />
  )
}

// Every drag-over commits a new draft, so an item re-renders only when its own
// props change instead of with the whole list.
const Item = memo(function Item(props: ItemProps) {
  // The whole row is the drag source; the pointer sensor ignores the menu.
  const { ref, isDragging, isDropTarget } = useSortable({
    id: props.profile.uid,
    index: props.index,
    disabled: props.saving,
  })

  return (
    <ItemContent
      {...props}
      itemRef={ref}
      dragging={isDragging}
      dropTarget={isDropTarget}
      draggable={!props.saving}
    />
  )
})

/** Count of enabled transforms and the menu that adds one. */
function TransformChainHeader({ chain }: { chain: TransformChainDraft }) {
  return (
    <div
      data-slot="profile-chain-editor-header"
      className="flex items-center justify-between gap-3"
    >
      <p className="text-on-surface-variant flex items-center gap-2 px-3 text-sm">
        <FormatListNumberedRounded className="size-5" aria-hidden />
        {m.profile_chain_editor_enabled_count({
          count: chain.enabled.length,
        })}
      </p>

      <TransformAddMenu
        available={chain.available}
        disabled={chain.actionsDisabled}
        onAdd={chain.handleAdd}
      />
    </div>
  )
}

/** Appears once the draft differs from the persisted chain. */
function TransformChainBar({ chain }: { chain: TransformChainDraft }) {
  return (
    <AnimatePresence>
      {chain.dataReady && chain.isChanged && (
        <AnimatedItem
          data-slot="profile-chain-editor-bar"
          className="bg-surface sticky bottom-0 z-10 rounded-full"
        >
          <div className="flex items-center justify-end gap-1 p-2">
            <Button
              onClick={chain.handleReset}
              disabled={chain.actionsDisabled}
            >
              {m.common_reset()}
            </Button>

            <Button
              variant="flat"
              onClick={chain.handleApply}
              disabled={chain.actionsDisabled}
            >
              {m.common_apply()}
            </Button>
          </div>
        </AnimatedItem>
      )}
    </AnimatePresence>
  )
}

/**
 * The sortable list of a transform chain draft owned by the caller. `header`
 * and `footer` are the caller's controls for the draft, such as the add menu
 * and the Apply bar.
 */
export function TransformChain({
  chain,
  traceStepOf,
  variant = 'card',
  empty,
  header,
  footer,
  className,
}: {
  chain: TransformChainDraft
  /** Builds the Inspect step key of a chain entry from its uid and position. */
  traceStepOf: (uid: string, index: number) => string
  variant?: TransformChainVariant
  /** Replaces the text of the empty chain. */
  empty?: { message: string; hint: string }
  header?: ReactNode
  footer?: ReactNode
  className?: string
}) {
  // Duplicated ids must never register multiple sortables.
  const readOnly = !chain.editable && !chain.dragging
  const Group = variant === 'step' ? StepGroup : SettingsGroup

  return (
    <section
      data-slot="profile-chain-editor"
      aria-busy={chain.saving}
      className={cn('relative flex min-w-0 flex-col gap-3', className)}
    >
      {header}

      {chain.saving && (
        <LinearProgress
          indeterminate
          className="h-1"
          aria-label={m.profile_chain_editor_apply_message()}
        />
      )}

      {chain.hasDuplicates && (
        <p role="alert" className="text-error flex items-start gap-2 text-sm">
          <WarningRounded className="mt-0.5 size-4 shrink-0" aria-hidden />
          {m.profile_chain_editor_duplicate_hint()}
        </p>
      )}

      <DragDropProvider
        sensors={WHOLE_ITEM_DRAG_SENSORS}
        onBeforeDragStart={(event) => {
          if (!chain.canDrag()) event.preventDefault()
        }}
        onDragStart={(_event, manager) => {
          // dnd-kit awaits a render between beforedragstart and dragstart and
          // its continuation re-checks neither the source nor the editor, so
          // a save or a correction can have landed in that window.
          if (!chain.canDrag()) {
            manager.actions.stop({ canceled: true })
            return
          }

          chain.beginDrag()
        }}
        onDragOver={(event) => {
          // Synchronous and unconditional: the optimistic sorting plugin only
          // reads defaultPrevented in a microtask, so this reliably stops it
          // from relocating React-owned nodes.
          event.preventDefault()

          chain.reorder((current) => move(current, event))
        }}
        onDragEnd={(event) => chain.endDrag(event.canceled)}
      >
        {chain.enabled.length === 0 ? (
          variant === 'step' ? (
            <StepGroup>
              <div
                data-slot="profile-chain-editor-empty"
                className={stepRowVariants()}
              >
                <StepRowIcon>
                  <PlaylistAddRounded />
                </StepRowIcon>

                <StepRowText
                  dim
                  title={empty?.message ?? m.profile_chain_editor_empty()}
                  sub={empty?.hint ?? m.profile_chain_editor_empty_hint()}
                />
              </div>
            </StepGroup>
          ) : (
            <EmptyState
              size="compact"
              tone="normal"
              data-slot="profile-chain-editor-empty"
              className="min-h-28 py-5"
              icon={FormatListNumberedRounded}
              message={empty?.message ?? m.profile_chain_editor_empty()}
              description={empty?.hint ?? m.profile_chain_editor_empty_hint()}
            />
          )
        ) : (
          <Group data-slot="profile-chain-editor-items">
            {chain.enabled.map((profile, index) =>
              readOnly ? (
                <ItemContent
                  key={`${profile.uid}-${index}`}
                  variant={variant}
                  profile={profile}
                  index={index}
                  count={chain.enabled.length}
                  traceStep={traceStepOf(profile.uid, index)}
                  saving
                  actionsDisabled
                  onMove={chain.handleMove}
                  onRemove={chain.handleRemove}
                />
              ) : (
                <Item
                  key={profile.uid}
                  variant={variant}
                  profile={profile}
                  index={index}
                  count={chain.enabled.length}
                  traceStep={traceStepOf(profile.uid, index)}
                  saving={chain.saving}
                  actionsDisabled={chain.actionsDisabled}
                  onMove={chain.handleMove}
                  onRemove={chain.handleRemove}
                />
              ),
            )}
          </Group>
        )}
      </DragDropProvider>

      {footer}
    </section>
  )
}

/** Ordered, sortable chain of enabled transforms with an explicit Apply. */
export function TransformChainList({
  transforms,
  taskKey,
  traceStepOf,
  onApply,
  className,
}: {
  /** The persisted chain, in order. */
  transforms: string[]
  taskKey: string
  /** Builds the Inspect step key of a chain entry from its uid and position. */
  traceStepOf: (uid: string, index: number) => string
  onApply: (transforms: string[]) => Promise<unknown>
  className?: string
}) {
  const chain = useTransformChainDraft({ transforms, taskKey, onApply })

  return (
    <TransformChain
      chain={chain}
      traceStepOf={traceStepOf}
      header={<TransformChainHeader chain={chain} />}
      footer={<TransformChainBar chain={chain} />}
      className={className}
    />
  )
}

import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import ManageSearchRounded from '~icons/material-symbols/manage-search-rounded'
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { AnimatePresence } from 'motion/react'
import { memo, type ReactNode } from 'react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { Button } from '@nyanpasu/ui/button'
import { cardVariants } from '@nyanpasu/ui/card'
import { LinearProgress } from '@nyanpasu/ui/progress'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import {
  sortableItemVariants,
  SortableOrder,
  useSortableTransition,
} from '../../_modules/sortable-item'
import { RowAction } from './compact-controls'
import { PROFILE_TYPE_NAMES } from './consts'
import {
  StepGroup,
  StepRowIcon,
  StepRowText,
  stepRowVariants,
} from './step-list'
import { TransformTypeBadge } from './transform-type-icon'
import { type TransformChainDraft } from './use-transform-chain-draft'
import { profileTypeOf, type TransformProfile } from './utils'

type ItemProps = {
  profile: TransformProfile
  index: number
  /** The Inspect step key of this row, matching the runtime node. */
  traceStep: string
  /** In the draft but not in the persisted chain. */
  unapplied: boolean
  saving: boolean
  actionsDisabled: boolean
  onRemove: (uid: string) => void
}

function ItemContent({
  profile,
  index,
  traceStep,
  unapplied,
  saving,
  actionsDisabled,
  onRemove,
  itemRef,
  dragging,
  draggable,
}: ItemProps & {
  itemRef?: (element: Element | null) => void
  dragging?: boolean
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
        sortableItemVariants({ draggable, dragging, disabled: saving }),
        'group/row flex min-h-14 min-w-0 items-center gap-3 py-1.5 pr-2 pl-3',
        // A transform that has just joined the chain grows into place.
        unapplied && 'animate-row-enter motion-reduce:animate-none',
      )}
    >
      <SortableOrder active data-slot="profile-chain-editor-order">
        {index + 1}
      </SortableOrder>

      <TransformTypeBadge type={profileTypeOf(profile)} />

      <StepRowText
        title={profile.name}
        sub={
          <>
            {PROFILE_TYPE_NAMES[profileTypeOf(profile)]}

            {unapplied && (
              <span
                data-slot="profile-chain-editor-unapplied"
                className="text-primary"
              >
                {' · '}
                {m.profile_chain_editor_unapplied()}
              </span>
            )}
          </>
        }
      />

      {/* Hover-capable pointers see the actions on hover or focus; touch has
          no hover, so they stay in the open there. */}
      <div
        data-slot="profile-chain-editor-row-actions"
        className={cn(
          'flex shrink-0 items-center',
          'transition-opacity motion-reduce:transition-none',
          '[@media(hover:hover)]:opacity-0',
          'group-focus-within/row:opacity-100 group-hover/row:opacity-100',
        )}
      >
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
    </div>
  )
}

// Every drag-over commits a new draft, so an item re-renders only when its own
// props change instead of with the whole list.
const Item = memo(function Item(props: ItemProps) {
  // The whole row is the drag source; the pointer sensor ignores the menu.
  const { ref, isDragging } = useSortable({
    id: props.profile.uid,
    index: props.index,
    disabled: props.saving,
    transition: useSortableTransition(),
  })

  return (
    <ItemContent
      {...props}
      itemRef={ref}
      dragging={isDragging}
      draggable={!props.saving}
    />
  )
})

/** Count of enabled transforms. */
export function TransformChainHeader({
  chain,
}: {
  chain: TransformChainDraft
}) {
  return (
    <p
      data-slot="profile-chain-editor-header"
      className="text-on-surface-variant px-3 text-sm"
    >
      {m.profile_chain_editor_enabled_count({ count: chain.enabled.length })}
    </p>
  )
}

/** What the draft changed against the persisted chain, as one line of text. */
function summaryOf({
  added,
  removed,
  reordered,
}: TransformChainDraft['changes']) {
  return [
    added > 0 && m.profile_chain_editor_summary_added({ count: added }),
    removed > 0 && m.profile_chain_editor_summary_removed({ count: removed }),
    reordered && m.profile_chain_editor_summary_reordered(),
  ]
    .filter(Boolean)
    .join(' · ')
}

/** Appears once the draft differs from the persisted chain. */
export function TransformChainBar({ chain }: { chain: TransformChainDraft }) {
  const summary = summaryOf(chain.changes)

  return (
    <AnimatePresence>
      {chain.dataReady && chain.isChanged && (
        <AnimatedItem
          data-slot="profile-chain-editor-bar"
          className="bg-surface sticky bottom-0 z-10 rounded-full"
        >
          <div className="flex items-center justify-end gap-1 p-2">
            {summary && (
              <p
                data-slot="profile-chain-editor-summary"
                className="text-on-surface-variant min-w-0 flex-1 truncate pl-3 text-xs"
              >
                {summary}
              </p>
            )}

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
 * and `footer` are the caller's controls for the draft, such as the enabled
 * count and the Apply bar.
 */
export function TransformChain({
  chain,
  traceStepOf,
  empty,
  header,
  footer,
  className,
}: {
  chain: TransformChainDraft
  /** Builds the Inspect step key of a chain entry from its uid and position. */
  traceStepOf: (uid: string, index: number) => string
  /** Replaces the text of the empty chain. */
  empty?: { message: string; hint: string }
  header?: ReactNode
  footer?: ReactNode
  className?: string
}) {
  // Duplicated ids must never register multiple sortables.
  const readOnly = !chain.editable && !chain.dragging

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
          <StepGroup data-slot="profile-chain-editor-items">
            {chain.enabled.map((profile, index) =>
              readOnly ? (
                <ItemContent
                  key={`${profile.uid}-${index}`}
                  profile={profile}
                  index={index}
                  traceStep={traceStepOf(profile.uid, index)}
                  unapplied={chain.isUnapplied(profile.uid)}
                  saving
                  actionsDisabled
                  onRemove={chain.handleRemove}
                />
              ) : (
                <Item
                  key={profile.uid}
                  profile={profile}
                  index={index}
                  traceStep={traceStepOf(profile.uid, index)}
                  unapplied={chain.isUnapplied(profile.uid)}
                  saving={chain.saving}
                  actionsDisabled={chain.actionsDisabled}
                  onRemove={chain.handleRemove}
                />
              ),
            )}
          </StepGroup>
        )}
      </DragDropProvider>

      {footer}
    </section>
  )
}

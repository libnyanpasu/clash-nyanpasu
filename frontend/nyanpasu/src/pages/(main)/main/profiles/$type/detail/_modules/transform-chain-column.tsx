import AddRounded from '~icons/material-symbols/add-rounded'
import FormatListNumberedRounded from '~icons/material-symbols/format-list-numbered-rounded'
import LayersOutlineRounded from '~icons/material-symbols/layers-outline-rounded'
import RemoveRounded from '~icons/material-symbols/remove-rounded'
import { memo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { pointerIntersection, type CollisionDetector } from '@dnd-kit/collision'
import { useDroppable } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { cn } from '@nyanpasu/utils'
import { profileTypeOf, type TransformProfile } from '../../_modules/utils'
import { PROFILE_TYPE_NAMES } from '../../../_modules/consts'

export type TransformChainLayout = 'stacked' | 'grouped'
type ColumnType = 'active' | 'inactive'

// A column accepts its empty space even when the dragged row overlaps a sibling
// column. Its items still win pointer collisions, regardless of centre distance.
const columnCollisionDetection: CollisionDetector = (context) => {
  const collision = pointerIntersection(context)
  return collision ? { ...collision, value: 0 } : null
}

type ItemProps = {
  profile: TransformProfile
  index: number
  column: ColumnType
  compact: boolean
  saving: boolean
  actionsDisabled: boolean
  onToggle: (uid: string) => void
}

function ItemContent({
  profile,
  index,
  column,
  compact,
  saving,
  actionsDisabled,
  onToggle,
  itemRef,
  dragging,
  dropTarget,
  draggable,
}: ItemProps & {
  itemRef?: (element: Element | null) => void
  dragging?: boolean
  dropTarget?: boolean
  draggable?: boolean
}) {
  const active = column === 'active'
  // Grouped candidates already sit under a heading naming their type.
  const showType = active || !compact

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
        'bg-surface-variant/40 flex min-w-0 items-center gap-3 rounded-2xl py-2.5 pr-2.5 pl-4 select-none',
        // Only colours transition: dnd-kit animates the item's position itself,
        // and animating shadows or transforms on every card is costly while the
        // columns reflow.
        'transition-colors motion-reduce:transition-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:outline-offset-2',
        compact && 'gap-2.5 rounded-xl py-2 pr-2 pl-3',
        draggable &&
          'hover:bg-surface-variant/70 cursor-grab touch-none active:cursor-grabbing',
        dragging && 'bg-secondary-container scale-[1.02] shadow-lg',
        dropTarget && 'ring-primary ring-2',
        saving && 'opacity-50',
      )}
    >
      {active && (
        <span
          data-slot="profile-chain-editor-order"
          className="bg-primary text-on-primary grid size-6 shrink-0 place-items-center rounded-full text-xs font-medium tabular-nums"
        >
          {index + 1}
        </span>
      )}

      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium [overflow-wrap:anywhere]">
          {profile.name}
        </p>
        {showType && (
          <span
            data-slot="profile-chain-editor-type"
            className="bg-secondary-container/60 text-on-secondary-container mt-1 inline-block rounded-md px-1.5 py-0.5 text-xs"
          >
            {PROFILE_TYPE_NAMES[profileTypeOf(profile)]}
          </span>
        )}
      </div>

      <Button
        type="button"
        icon
        disabled={actionsDisabled}
        data-slot="profile-chain-editor-toggle"
        aria-label={
          active
            ? m.profile_chain_editor_remove_item({ name: profile.name })
            : m.profile_chain_editor_add_item({ name: profile.name })
        }
        className="text-on-surface-variant size-9 shrink-0 focus-visible:outline-2 focus-visible:outline-offset-2"
        onClick={() => onToggle(profile.uid)}
      >
        {active ? (
          <RemoveRounded className="size-5" aria-hidden />
        ) : (
          <AddRounded className="size-5" aria-hidden />
        )}
      </Button>
    </div>
  )
}

// Every drag-over commits a new draft, so an item re-renders only when its own
// props change instead of with the whole editor.
const Item = memo(function Item(props: ItemProps) {
  // The whole card is the drag source; the pointer sensor ignores the toggle.
  const { ref, isDragging, isDropTarget } = useSortable({
    id: props.profile.uid,
    index: props.index,
    // move() resolves the target column using this group as a draft record key.
    group: props.column,
    type: 'item',
    accept: ['item'],
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

export const TransformChainColumn = memo(function TransformChainColumn({
  profiles,
  type,
  saving,
  readOnly,
  layout,
  actionsDisabled,
  onToggle,
}: {
  profiles: TransformProfile[]
  type: ColumnType
  saving: boolean
  readOnly: boolean
  layout: TransformChainLayout
  actionsDisabled: boolean
  onToggle: (uid: string) => void
}) {
  const { ref, isDropTarget } = useDroppable({
    id: type,
    type: 'column',
    accept: ['item'],
    collisionDetector: columnCollisionDetection,
    disabled: saving || readOnly,
  })
  const active = type === 'active'
  const compact = layout === 'grouped'
  const title = active
    ? m.profile_chain_editor_active_column()
    : m.profile_chain_editor_inactive_column()

  return (
    <section
      ref={ref}
      data-slot="profile-chain-editor-column"
      data-column={type}
      data-drop-target={isDropTarget || undefined}
      aria-label={title}
      className={cn(
        'min-w-0 rounded-3xl p-4',
        'transition-colors motion-reduce:transition-none',
        active ? 'bg-secondary-container/20' : 'bg-surface-variant/20',
        isDropTarget && 'bg-secondary-container/40 ring-primary ring-2',
      )}
    >
      <div data-slot="profile-chain-editor-column-header" className="mb-4">
        <div className="flex items-center gap-2">
          {active ? (
            <FormatListNumberedRounded
              className="text-primary size-5"
              aria-hidden
            />
          ) : (
            <LayersOutlineRounded className="text-primary size-5" aria-hidden />
          )}
          <h3 className="text-base font-medium">{title}</h3>
          <span className="bg-surface-variant text-on-surface-variant rounded-lg px-2 py-0.5 text-xs tabular-nums">
            {profiles.length}
          </span>
        </div>
        <p className="text-on-surface-variant mt-1 text-xs">
          {active
            ? m.profile_chain_editor_active_hint()
            : m.profile_chain_editor_inactive_hint()}
        </p>
      </div>

      <div
        data-slot="profile-chain-editor-column-items"
        className={cn(
          'grid min-h-24 content-start gap-3 pb-3',
          !active && !compact && 'sm:grid-cols-2',
          compact && 'gap-1.5',
        )}
      >
        {profiles.length === 0 ? (
          <div
            data-slot="profile-chain-editor-empty"
            className="text-on-surface-variant col-span-full flex min-h-28 flex-col items-center justify-center gap-2 px-3 py-5 text-center"
          >
            <span className="bg-surface-variant text-primary mb-1 grid size-12 place-items-center rounded-2xl">
              {active ? (
                <AddRounded className="size-6" aria-hidden />
              ) : (
                <LayersOutlineRounded className="size-6" aria-hidden />
              )}
            </span>
            <p className="text-on-surface text-sm font-medium">
              {active
                ? m.profile_chain_editor_active_empty()
                : m.profile_chain_editor_inactive_empty()}
            </p>
            <p className="text-xs">
              {active
                ? m.profile_chain_editor_active_empty_hint()
                : m.profile_chain_editor_inactive_empty_hint()}
            </p>
          </div>
        ) : (
          profiles.map((profile, index) => (
            <div
              key={readOnly ? `${profile.uid}-${index}` : profile.uid}
              className="min-w-0"
            >
              {compact &&
                !active &&
                (index === 0 ||
                  profileTypeOf(profiles[index - 1]!) !==
                    profileTypeOf(profile)) && (
                  // Consecutive groups preserve the sortable indexes even when
                  // a draft moves a candidate between different script types.
                  <p
                    data-slot="profile-chain-editor-type-heading"
                    className={cn(
                      'text-primary px-1 pb-2 text-xs font-medium tracking-wide',
                      index === 0 ? 'pt-0' : 'pt-3',
                    )}
                  >
                    {PROFILE_TYPE_NAMES[profileTypeOf(profile)]}
                  </p>
                )}
              {readOnly ? (
                // Duplicated ids must never register multiple sortables.
                <ItemContent
                  profile={profile}
                  index={index}
                  column={type}
                  compact={compact}
                  saving
                  actionsDisabled
                  onToggle={onToggle}
                />
              ) : (
                <Item
                  profile={profile}
                  index={index}
                  column={type}
                  compact={compact}
                  saving={saving}
                  actionsDisabled={actionsDisabled}
                  onToggle={onToggle}
                />
              )}
            </div>
          ))
        )}
      </div>
    </section>
  )
})

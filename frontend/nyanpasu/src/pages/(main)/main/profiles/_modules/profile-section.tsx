import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import { PropsWithChildren } from 'react'
import { m } from '@/paraglide/messages'
import { pointerIntersection } from '@dnd-kit/collision'
import { useDroppable } from '@dnd-kit/react'
import type { FolderId } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import FolderActionsMenu from './folder-actions-menu'
import {
  collapsedSectionId,
  PROFILE_DRAG_TYPE,
  SECTION_DRAG_TYPE,
  sectionId,
} from './resolve-profile-drop'

/**
 * dnd-kit's `CollisionPriority.Lowest`, which `@dnd-kit/abstract` exports but
 * this app does not depend on directly. A card under the pointer wins over the
 * section around it.
 */
const SECTION_COLLISION_PRIORITY = 0

export type ProfileSectionHeading = {
  title: string
  count: number
  collapsed: boolean
  handleToggle: () => void
}

/**
 * One group of profile cards: a folder, or the profiles in none. It is a drop
 * target for a card dragged over its gaps and, when it has no cards, over its
 * empty zone. Without a heading it is a plain grid.
 */
export default function ProfileSection({
  folder,
  heading,
  empty,
  children,
}: PropsWithChildren<{
  /** The folder this section shows, `null` for the profiles in none. */
  folder: FolderId | null
  heading?: ProfileSectionHeading
  /** Whether the section has no cards. */
  empty: boolean
}>) {
  const collapsed = heading?.collapsed ?? false

  // A collapsed section shows no cards, so a dragged card cannot be previewed
  // in it; its heading takes the card as a drop at the end instead.
  const { ref, isDropTarget } = useDroppable({
    id: collapsed ? collapsedSectionId(folder) : sectionId(folder),
    type: SECTION_DRAG_TYPE,
    accept: PROFILE_DRAG_TYPE,
    collisionPriority: SECTION_COLLISION_PRIORITY,
    collisionDetector: pointerIntersection,
  })

  return (
    <section
      ref={ref}
      className="flex flex-col gap-3"
      data-slot="profiles-folder-section"
      data-folder={folder ?? undefined}
      data-drop-target={isDropTarget || undefined}
    >
      {heading && (
        <div
          className="flex items-center gap-2 px-1"
          data-slot="profiles-folder-section-heading"
        >
          <button
            type="button"
            className={cn(
              'text-on-surface flex min-w-0 items-center gap-2 rounded-full py-1 pr-3 pl-1',
              'hover:bg-surface-variant/60 text-left font-medium',
            )}
            aria-expanded={!heading.collapsed}
            onClick={heading.handleToggle}
            data-slot="profiles-folder-section-toggle"
          >
            <ExpandMoreRounded
              className={cn(
                'size-6 shrink-0 transition-transform',
                heading.collapsed && '-rotate-90',
              )}
              aria-hidden
            />

            <span className="truncate">{heading.title}</span>
          </button>

          <span
            className="text-on-surface-variant text-sm"
            data-slot="profiles-folder-section-count"
          >
            {heading.count}
          </span>

          {folder && (
            <div className="ml-auto">
              <FolderActionsMenu folder={folder} />
            </div>
          )}
        </div>
      )}

      {!collapsed &&
        (empty ? (
          <div
            className={cn(
              'border-outline-variant text-on-surface-variant rounded-2xl border-2 border-dashed',
              'flex h-24 items-center justify-center text-sm',
              isDropTarget && 'border-primary bg-primary/10',
            )}
            data-slot="profiles-folder-section-empty"
          >
            {m.profile_folder_empty_hint()}
          </div>
        ) : (
          <div
            className="grid content-start gap-3 md:grid-cols-2 xl:grid-cols-3"
            data-slot="profiles-grid"
          >
            {children}
          </div>
        ))}
    </section>
  )
}

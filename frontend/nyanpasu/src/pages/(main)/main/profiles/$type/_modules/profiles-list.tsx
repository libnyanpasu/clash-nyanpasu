import { isEqual } from 'es-toolkit'
import { ComponentProps, useRef } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useProfile } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { ProfileType } from '../../_modules/consts'
import { GridViewProfile } from '../../_modules/profile-card'
import ProfileSections from '../../_modules/profile-sections'
import {
  NoMoreProfiles,
  useListOverflows,
} from '../../_modules/profiles-list-footer'
import { categoryProfiles } from '../../_modules/utils'
import { Route as IndexRoute } from '../index'

export default function ProfilesList(
  props: Omit<ComponentProps<'div'>, 'children'>,
) {
  const { type } = IndexRoute.useParams()

  return type === ProfileType.Profile ? (
    <ProfileSections {...props} />
  ) : (
    <TransformList {...props} />
  )
}

/** The transforms of one kind, in a plain list that has no folders. */
function TransformList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { type } = IndexRoute.useParams()

  const {
    query: { data: profiles },
    sort,
  } = useProfile()

  const filteredProfiles =
    categoryProfiles(profiles?.items)[type as ProfileType] ?? []

  const globalTransforms = new Set(profiles?.global_transforms)

  const gridRef = useRef<HTMLDivElement>(null)

  const overflows = useListOverflows(gridRef, filteredProfiles.length)

  if (filteredProfiles.length === 0) {
    return (
      <EmptyState className="flex-1" message={m.profile_empty_list_message()} />
    )
  }

  return (
    <>
      <div
        className={cn('flex min-h-full flex-1 flex-col gap-4')}
        data-slot="profiles-list"
        {...props}
      >
        <DragDropProvider
          sensors={WHOLE_ITEM_DRAG_SENSORS}
          onDragEnd={(event) => {
            const filteredUids = filteredProfiles.map((profile) => profile.uid)

            const nextFilteredUids = move(filteredUids, event)

            if (isEqual(filteredUids, nextFilteredUids)) {
              return
            }

            // reorder_profiles_by_list requires a FULL ordering of every
            // profile (the actor rejects partial lists). Splice the reordered
            // filtered uids back into the full items order, keeping items from
            // other tabs at their original positions.
            const filteredSet = new Set(filteredUids)
            let cursor = 0
            const fullOrder = (profiles?.items ?? []).map((item) =>
              filteredSet.has(item.uid) ? nextFilteredUids[cursor++] : item.uid,
            )

            sort.mutate(fullOrder)
          }}
        >
          <div
            ref={gridRef}
            className={cn(
              'grid content-start gap-3',
              'md:grid-cols-2',
              'xl:grid-cols-3',
              className,
            )}
            data-slot="profiles-grid"
          >
            {filteredProfiles.map((profile, index) => (
              <GridViewProfile
                key={profile.uid}
                profile={profile}
                profiles={profiles?.items ?? []}
                index={index}
                isGlobal={globalTransforms.has(profile.uid)}
              />
            ))}
          </div>
        </DragDropProvider>

        <div className="flex-1" />
      </div>

      {overflows && <NoMoreProfiles />}
    </>
  )
}

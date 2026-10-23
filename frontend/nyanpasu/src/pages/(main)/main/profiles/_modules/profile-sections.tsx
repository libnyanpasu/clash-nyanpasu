import { ComponentProps, useRef } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/query'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useFolderCollapsed } from '../../_modules/use-folder-collapsed'
import { GridViewProfile } from './profile-card'
import ProfileSection from './profile-section'
import { NoMoreProfiles, useListOverflows } from './profiles-list-footer'
import { sectionId } from './resolve-profile-drop'
import { useProfileLayout } from './use-profile-layout'
import { useProfilesDnd } from './use-profiles-dnd'

const COLLAPSED_KV_KEY = 'profiles-folder-collapsed'

/**
 * The Config profiles in their folders, then the ones in none. Without
 * folders it is a plain grid with no heading.
 */
export default function ProfileSections({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const {
    query: { data: profiles },
  } = useProfile()

  const { layout } = useProfileLayout()
  const { items } = useProfilesDnd()
  const { isCollapsed, toggle } = useFolderCollapsed(COLLAPSED_KV_KEY)

  const configs = new Map<string, ProfileItem_Serialize>(
    (profiles?.items ?? [])
      .filter((item) => item.type === 'config')
      .map((item) => [item.uid, item]),
  )

  const globalTransforms = new Set(profiles?.global_transforms)

  // The current profile is listed first, above every folder, and not in the
  // section of its folder.
  const active = profiles?.current ? configs.get(profiles.current) : undefined

  const listRef = useRef<HTMLDivElement>(null)

  const overflows = useListOverflows(listRef, configs.size)

  if (configs.size === 0) {
    return (
      <EmptyState className="flex-1" message={m.profile_empty_list_message()} />
    )
  }

  const folders = layout?.folders ?? []

  const renderSection = (
    folder: (typeof folders)[number] | null,
    title: string | undefined,
  ) => {
    const group = sectionId(folder?.id ?? null)
    const key = folder?.id ?? 'ungrouped'
    const sectionProfiles = (items[group] ?? []).flatMap(
      (uid) => configs.get(uid) ?? [],
    )

    // Without folders, a list of just the active profile has no use for a
    // place to drop one.
    if (!folder && folders.length === 0 && sectionProfiles.length === 0) {
      return null
    }

    return (
      <ProfileSection
        key={key}
        folder={folder?.id ?? null}
        empty={sectionProfiles.length === 0}
        heading={
          title === undefined
            ? undefined
            : {
                title,
                count: sectionProfiles.length,
                collapsed: isCollapsed(key),
                handleToggle: () => toggle(key),
              }
        }
      >
        {sectionProfiles.map((profile, index) => (
          <GridViewProfile
            key={profile.uid}
            profile={profile}
            profiles={profiles?.items ?? []}
            group={group}
            index={index}
            isGlobal={globalTransforms.has(profile.uid)}
          />
        ))}
      </ProfileSection>
    )
  }

  return (
    <>
      <div
        className="flex min-h-full flex-1 flex-col gap-4"
        data-slot="profiles-list"
        {...props}
      >
        <div
          ref={listRef}
          className={cn(
            'flex flex-col',
            folders.length > 0 ? 'gap-6' : 'gap-3',
            className,
          )}
          data-slot="profiles-sections"
        >
          {active && (
            <div
              className="grid gap-3 md:grid-cols-2 xl:grid-cols-3"
              data-slot="profiles-active"
            >
              <GridViewProfile
                profile={active}
                profiles={profiles?.items ?? []}
                index={0}
                isGlobal={globalTransforms.has(active.uid)}
              />
            </div>
          )}

          {folders.map((folder) => renderSection(folder, folder.name))}

          {renderSection(
            null,
            folders.length > 0
              ? m.profile_folder_section_ungrouped()
              : undefined,
          )}
        </div>

        <div className="flex-1" />
      </div>

      {overflows && <NoMoreProfiles />}
    </>
  )
}

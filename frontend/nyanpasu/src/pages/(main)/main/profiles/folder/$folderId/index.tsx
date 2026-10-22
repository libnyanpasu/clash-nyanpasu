import { useRef } from 'react'
import { useProfile } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import ImportButton from '../../_modules/import-button'
import { GridViewProfile } from '../../_modules/profile-card'
import ProfileQuickImport from '../../_modules/profile-quick-import'
import ProfileSection from '../../_modules/profile-section'
import {
  NoMoreProfiles,
  useListOverflows,
} from '../../_modules/profiles-list-footer'
import { sectionId } from '../../_modules/resolve-profile-drop'
import { useProfileLayout } from '../../_modules/use-profile-layout'
import { useProfilesDnd } from '../../_modules/use-profiles-dnd'
import FolderEmptyState from './_modules/folder-empty-state'
import FolderHeader from './_modules/folder-header'
import FolderNotFound from './_modules/folder-not-found'

export const Route = createFileRoute('/(main)/main/profiles/folder/$folderId/')(
  {
    component: RouteComponent,
  },
)

function RouteComponent() {
  const { folderId } = Route.useParams()

  const { layout } = useProfileLayout()

  const {
    query: { data: profiles },
  } = useProfile()

  const { items } = useProfilesDnd()

  const listRef = useRef<HTMLDivElement>(null)

  const folder = layout?.folders.find((item) => item.id === folderId)

  // The current profile is listed first and stays out of the section, as on
  // the All profiles page.
  const active = folder?.members.includes(profiles?.current ?? '')
    ? profiles?.items.find((item) => item.uid === profiles?.current)
    : undefined

  const members = new Map(
    (profiles?.items ?? []).map((item) => [item.uid, item]),
  )

  const sectionProfiles = (items[sectionId(folderId)] ?? []).flatMap(
    (uid) => members.get(uid) ?? [],
  )

  const globalTransforms = new Set(profiles?.global_transforms)

  const overflows = useListOverflows(
    listRef,
    sectionProfiles.length + (active ? 1 : 0),
  )

  // While the profiles load there is no layout, and no way to tell a folder
  // that is missing from one not yet known.
  if (!layout) {
    return null
  }

  if (!folder) {
    return <FolderNotFound />
  }

  return (
    <>
      <FolderHeader folder={folder} />

      <div className="px-4 pb-4">
        <ProfileQuickImport folder={folderId} />
      </div>

      {folder.members.length === 0 ? (
        <FolderEmptyState />
      ) : (
        <div
          ref={listRef}
          className="flex min-h-0 flex-1 flex-col gap-3 p-4 pt-0"
          data-slot="profiles-list"
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

          {sectionProfiles.length > 0 && (
            <ProfileSection folder={folderId} empty={false}>
              {sectionProfiles.map((profile, index) => (
                <GridViewProfile
                  key={profile.uid}
                  profile={profile}
                  profiles={profiles?.items ?? []}
                  group={sectionId(folderId)}
                  index={index}
                  isGlobal={globalTransforms.has(profile.uid)}
                />
              ))}
            </ProfileSection>
          )}
        </div>
      )}

      {overflows && <NoMoreProfiles />}

      <ImportButton kind="file" folder={folderId} />
    </>
  )
}

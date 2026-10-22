import { isEqual } from 'es-toolkit'
import { PropsWithChildren, useMemo, useRef, useState } from 'react'
import { m } from '@/paraglide/messages'
import { Accessibility } from '@dnd-kit/dom'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useProfile } from '@nyanpasu/query'
import {
  appendDestinationOf,
  FOLDER_DRAG_TYPE,
  folderOfSidebarId,
  PROFILE_DRAG_TYPE,
  resolveProfileDrop,
  sectionId,
  sectionItemsOf,
  sidebarFolderId,
  withoutProfile,
  type SectionItems,
} from './resolve-profile-drop'
import { useProfileLayout } from './use-profile-layout'
import { ProfilesDndContext, type ProfilesDnd } from './use-profiles-dnd'

type Announcements = NonNullable<
  NonNullable<ConstructorParameters<typeof Accessibility>[1]>['announcements']
>

/**
 * The one drag-and-drop context of the profiles page, so a card can be
 * dropped on the sidebar. It owns the preview of a card drag and turns a drop
 * into at most one layout commit.
 */
export default function ProfilesDndProvider({ children }: PropsWithChildren) {
  const { layout, moveProfile, reorderFolders } = useProfileLayout()

  const {
    query: { data: profiles },
  } = useProfile()

  // The current profile is listed above the sections, so it has no place in
  // them to drag over or to drop on.
  const currentUid = profiles?.current

  const current = useMemo(
    () => (layout ? withoutProfile(sectionItemsOf(layout), currentUid) : {}),
    [layout, currentUid],
  )

  // Handlers and announcements read the latest values through refs: a drag
  // outlives the render that started it.
  const currentRef = useRef(current)
  currentRef.current = current

  const layoutRef = useRef(layout)
  layoutRef.current = layout

  const profilesRef = useRef(profiles)
  profilesRef.current = profiles

  const [preview, setPreview] = useState<SectionItems | null>(null)
  const previewRef = useRef<SectionItems | null>(null)
  // Tells a drop's cleanup whether a newer drag has started since.
  const dragCountRef = useRef(0)

  const showPreview = (next: SectionItems | null) => {
    previewRef.current = next
    setPreview(next)
  }

  const announcements = useMemo(() => {
    const profileName = (id: string) =>
      profilesRef.current?.items.find((item) => item.uid === id)?.name ?? id

    const folderName = (key: string | null) =>
      layoutRef.current?.folders.find((folder) => sectionId(folder.id) === key)
        ?.name ?? m.profile_folder_section_ungrouped()

    const nameOf = (id: string, type: unknown) =>
      type === FOLDER_DRAG_TYPE
        ? (layoutRef.current?.folders.find(
            (folder) => sidebarFolderId(folder.id) === id,
          )?.name ?? id)
        : profileName(id)

    const messages: Announcements = {
      dragstart: ({ operation: { source } }) =>
        source
          ? m.profile_folder_announce_pickup({
              name: nameOf(String(source.id), source.type),
            })
          : undefined,
      dragover: ({ operation: { source, target } }) => {
        if (!source || !target) return undefined

        const name = nameOf(String(source.id), source.type)
        const targetId = String(target.id)

        if (source.type !== PROFILE_DRAG_TYPE) return undefined

        const destination = appendDestinationOf(targetId)

        if (destination !== undefined) {
          return m.profile_folder_announce_over({
            name,
            folder: folderName(sectionId(destination)),
          })
        }

        for (const [key, uids] of Object.entries(
          previewRef.current ?? currentRef.current,
        )) {
          const index = uids.indexOf(String(source.id))

          if (index !== -1) {
            return m.profile_folder_announce_move({
              profile: name,
              folder: folderName(key),
              position: index + 1,
              total: uids.length,
            })
          }
        }

        return undefined
      },
      dragend: ({ operation: { source }, canceled }) => {
        if (!source) return undefined

        const name = nameOf(String(source.id), source.type)

        return canceled
          ? m.profile_folder_announce_cancel({ name })
          : m.profile_folder_announce_drop({ name })
      },
    }

    return messages
  }, [])

  const value = useMemo<ProfilesDnd>(
    () => ({ items: preview ?? current }),
    [preview, current],
  )

  return (
    <ProfilesDndContext.Provider value={value}>
      <DragDropProvider
        plugins={(defaults) => [
          ...defaults.filter((plugin) => plugin !== Accessibility),
          Accessibility.configure({
            announcements,
            screenReaderInstructions: {
              draggable: m.profile_folder_drag_instructions(),
            },
          }),
        ]}
        onDragStart={({ operation: { source } }) => {
          if (source?.type !== PROFILE_DRAG_TYPE) return

          dragCountRef.current += 1
          showPreview(currentRef.current)
        }}
        onDragOver={(event) => {
          const { source, target } = event.operation

          if (source?.type !== PROFILE_DRAG_TYPE || !target) return

          // The list renders the preview itself, so the sortable plugin must
          // not move the card's element as well: the plugin's move and React's
          // commit would fight over the same node and the list would crash.
          event.preventDefault()

          // A sidebar item or a collapsed heading takes the card without
          // showing it in a section.
          if (appendDestinationOf(String(target.id)) !== undefined) {
            return
          }

          if (previewRef.current) {
            showPreview(move(previewRef.current, event))
          }
        }}
        onDragEnd={(event) => {
          const { source, target } = event.operation

          if (!source) return

          if (source.type === FOLDER_DRAG_TYPE) {
            if (event.canceled) return

            const ids = (layoutRef.current?.folders ?? []).map((folder) =>
              sidebarFolderId(folder.id),
            )

            const reordered = move(ids, event)

            if (!isEqual(reordered, ids)) {
              reorderFolders(
                reordered.flatMap((id) => folderOfSidebarId(id) ?? []),
              )
            }

            return
          }

          if (source.type !== PROFILE_DRAG_TYPE) return

          const drop =
            event.canceled || !layoutRef.current
              ? null
              : resolveProfileDrop({
                  uid: String(source.id),
                  targetId: target ? String(target.id) : undefined,
                  items: previewRef.current,
                  layout: layoutRef.current,
                })

          if (!drop) {
            showPreview(null)
            return
          }

          // A drop on a sidebar item has no place in a section to preview.
          if (drop.index === undefined) {
            showPreview(null)
          }

          const dragCount = dragCountRef.current

          // The preview stays until the commit has updated the list, so the
          // card does not jump back for a frame.
          moveProfile(String(source.id), drop.destination, drop.index).finally(
            () => {
              if (dragCountRef.current === dragCount) {
                showPreview(null)
              }
            },
          )
        }}
      >
        {children}
      </DragDropProvider>
    </ProfilesDndContext.Provider>
  )
}

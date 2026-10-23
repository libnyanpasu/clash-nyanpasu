import { PropsWithChildren, useMemo, useRef, useState } from 'react'
import { m } from '@/paraglide/messages'
import { Accessibility } from '@dnd-kit/dom'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { reorderGroupFoldersInLayout } from './group-layout'
import {
  FOLDER_DRAG_TYPE,
  folderDragIdsOf,
  folderOfDragId,
  folderOfHeadingId,
  GROUP_DRAG_TYPE,
  groupOfDragId,
  resolveGroupDrop,
  sectionId,
  sectionItemsOf,
  type SectionItems,
} from './resolve-group-drop'
import { useGroupLayout } from './use-group-layout'
import { ProxiesDndContext, type ProxiesDnd } from './use-proxies-dnd'

type Announcements = NonNullable<
  NonNullable<ConstructorParameters<typeof Accessibility>[1]>['announcements']
>

/** A folder heading takes a group without showing it in a list. */
const isHeading = (targetId: string) =>
  folderOfHeadingId(targetId) !== undefined

/**
 * The one drag-and-drop context of the Proxies sidebar. It owns the preview of
 * a drag, which the list renders itself, and turns a drop into at most one
 * layout commit. Escape cancels without a commit.
 */
export default function ProxiesDndProvider({ children }: PropsWithChildren) {
  const { layout, commitLayout, moveGroup } = useGroupLayout()

  const current = useMemo(
    () => (layout ? sectionItemsOf(layout) : {}),
    [layout],
  )
  const currentFolders = useMemo(
    () => (layout ? folderDragIdsOf(layout) : []),
    [layout],
  )

  // Handlers and announcements read the latest values through refs: a drag
  // outlives the render that started it.
  const currentRef = useRef(current)
  currentRef.current = current

  const currentFoldersRef = useRef(currentFolders)
  currentFoldersRef.current = currentFolders

  const layoutRef = useRef(layout)
  layoutRef.current = layout

  const [preview, setPreview] = useState<SectionItems | null>(null)
  const previewRef = useRef<SectionItems | null>(null)
  const [foldersPreview, setFoldersPreview] = useState<string[] | null>(null)
  const foldersPreviewRef = useRef<string[] | null>(null)
  // Tells a drop's cleanup whether a newer drag has started since.
  const dragCountRef = useRef(0)

  const showPreview = (next: SectionItems | null) => {
    previewRef.current = next
    setPreview(next)
  }

  const showFoldersPreview = (next: string[] | null) => {
    foldersPreviewRef.current = next
    setFoldersPreview(next)
  }

  const announcements = useMemo(() => {
    const folderName = (key: string | null) =>
      layoutRef.current?.folders.find((folder) => sectionId(folder.id) === key)
        ?.name ?? m.profile_folder_section_ungrouped()

    const nameOf = (id: string, type: unknown) =>
      type === FOLDER_DRAG_TYPE
        ? (layoutRef.current?.folders.find(
            (folder) => folder.id === folderOfDragId(id),
          )?.name ?? id)
        : (groupOfDragId(id) ?? id)

    const messages: Announcements = {
      dragstart: ({ operation: { source } }) =>
        source
          ? m.profile_folder_announce_pickup({
              name: nameOf(String(source.id), source.type),
            })
          : undefined,
      dragover: ({ operation: { source, target } }) => {
        if (!source || !target || source.type !== GROUP_DRAG_TYPE) {
          return undefined
        }

        const name = nameOf(String(source.id), source.type)
        const targetId = String(target.id)

        if (isHeading(targetId)) {
          const folder = layoutRef.current?.folders.find(
            (item) => item.id === folderOfHeadingId(targetId),
          )

          return m.profile_folder_announce_over({
            name,
            folder: folder?.name ?? m.profile_folder_section_ungrouped(),
          })
        }

        for (const [key, ids] of Object.entries(
          previewRef.current ?? currentRef.current,
        )) {
          const index = ids.indexOf(String(source.id))

          if (index !== -1) {
            return m.profile_folder_announce_move({
              profile: name,
              folder: folderName(key),
              position: index + 1,
              total: ids.length,
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

  const value = useMemo<ProxiesDnd>(
    () => ({
      items: preview ?? current,
      folders: foldersPreview ?? currentFolders,
    }),
    [preview, current, foldersPreview, currentFolders],
  )

  return (
    <ProxiesDndContext.Provider value={value}>
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
          if (source?.type === GROUP_DRAG_TYPE) {
            dragCountRef.current += 1
            showPreview(currentRef.current)
          } else if (source?.type === FOLDER_DRAG_TYPE) {
            dragCountRef.current += 1
            showFoldersPreview(currentFoldersRef.current)
          }
        }}
        onDragOver={(event) => {
          const { source, target } = event.operation

          if (!target) return

          // The list renders the preview itself, so the sortable plugin must
          // not move the element as well: the plugin's move and React's
          // commit would fight over the same node and the list would crash.
          if (source?.type === GROUP_DRAG_TYPE) {
            event.preventDefault()

            if (isHeading(String(target.id))) return

            if (previewRef.current) {
              showPreview(move(previewRef.current, event))
            }
          } else if (source?.type === FOLDER_DRAG_TYPE) {
            event.preventDefault()

            if (foldersPreviewRef.current) {
              showFoldersPreview(move(foldersPreviewRef.current, event))
            }
          }
        }}
        onDragEnd={(event) => {
          const { source, target } = event.operation

          if (!source) return

          const dragCount = dragCountRef.current

          // The preview stays until the commit has updated the list, so the
          // item does not jump back for a frame.
          const settle = (clear: () => void) => (commit: Promise<boolean>) =>
            commit.finally(() => {
              if (dragCountRef.current === dragCount) clear()
            })

          if (source.type === FOLDER_DRAG_TYPE) {
            const ids = foldersPreviewRef.current
            const latest = layoutRef.current

            if (event.canceled || !ids || !latest) {
              showFoldersPreview(null)
              return
            }

            settle(() => showFoldersPreview(null))(
              commitLayout(
                reorderGroupFoldersInLayout(
                  latest,
                  ids.flatMap((id) => folderOfDragId(id) ?? []),
                ),
              ),
            )

            return
          }

          if (source.type !== GROUP_DRAG_TYPE) return

          const name = groupOfDragId(String(source.id))

          const drop =
            event.canceled || !name
              ? null
              : resolveGroupDrop({
                  name,
                  targetId: target ? String(target.id) : undefined,
                  items: previewRef.current,
                })

          if (!name || !drop) {
            showPreview(null)
            return
          }

          // A drop on a heading has no place in a list to preview.
          if (drop.index === undefined) {
            showPreview(null)
          }

          settle(() => showPreview(null))(
            moveGroup(name, drop.destination, drop.index),
          )
        }}
      >
        {children}
      </DragDropProvider>
    </ProxiesDndContext.Provider>
  )
}

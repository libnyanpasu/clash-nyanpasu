import CloudDownloadRounded from '~icons/material-symbols/cloud-download-rounded'
import LinkRounded from '~icons/material-symbols/link-rounded'
import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded'
import UploadFileRounded from '~icons/material-symbols/upload-file-rounded'
import { useEffect, useState } from 'react'
import {
  FabMenu,
  FabMenuContent,
  FabMenuItem,
  FabMenuTrigger,
} from '@nyanpasu/ui/fab-menu'
import { m } from '@/paraglide/messages'
import { type FolderId } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import CreateProfileModal from './create-profile-modal'
import type { CreateKind, CreateSource } from './create-profile-schema'

const SOURCE_ITEMS = [
  {
    source: 'remote',
    icon: CloudDownloadRounded,
    label: () => m.profile_source_remote(),
  },
  {
    source: 'local',
    icon: UploadFileRounded,
    label: () => m.profile_source_local(),
  },
  {
    source: 'external',
    icon: LinkRounded,
    label: () => m.profile_source_external(),
  },
] as const satisfies readonly {
  source: CreateSource
  icon: typeof NoteStackAddRounded
  label: () => string
}[]

export default function ImportButton({
  kind,
  folder,
  localImportRequested = false,
  onLocalImportHandled,
}: {
  /** Initial kind of the create modal. */
  kind: CreateKind
  /** Folder a created Config joins; omitted creates it ungrouped. */
  folder?: FolderId | null
  /** Opens the modal on the local-file source, e.g. from the tray action. */
  localImportRequested?: boolean
  /** Called once the modal opened for `localImportRequested` closes. */
  onLocalImportHandled?: () => void
}) {
  const [open, setOpen] = useState(false)

  const [source, setSource] = useState<CreateSource>('remote')

  useEffect(() => {
    if (!localImportRequested) {
      return
    }

    // wait for the route transition before opening the modal
    const timeout = setTimeout(() => {
      setSource('local')
      setOpen(true)
    }, 150)

    return () => {
      clearTimeout(timeout)
    }
  }, [localImportRequested])

  const handleOpenChange = (value: boolean) => {
    setOpen(value)

    if (!value && localImportRequested) {
      onLocalImportHandled?.()
    }
  }

  return (
    <div
      className={cn(
        'absolute right-4 ml-auto w-fit',
        // The top position is calculated based on the viewport height and the heights of other components (header, tabs, etc.)
        // DO NOT change these values unless you know what you are doing
        'top-[calc(100vh-40px-64px-72px)]',
        'sm:top-[calc(100vh-40px-48px-72px)]',
        'transition-[bottom] duration-500',
        'group-data-[scroll-direction=down]/profiles-content:-bottom-18',
      )}
    >
      <FabMenu>
        <FabMenuContent>
          {SOURCE_ITEMS.map(({ source: item, icon: Icon, label }) => (
            <FabMenuItem
              key={item}
              icon={<Icon className="size-6" />}
              onSelect={() => {
                setSource(item)
                setOpen(true)
              }}
            >
              {label()}
            </FabMenuItem>
          ))}
        </FabMenuContent>

        <FabMenuTrigger
          icon={<NoteStackAddRounded className="size-6" />}
          aria-label={m.profile_import_title()}
        />
      </FabMenu>

      <CreateProfileModal
        open={open}
        onOpenChange={handleOpenChange}
        kind={kind}
        folder={folder}
        source={source}
      />
    </div>
  )
}

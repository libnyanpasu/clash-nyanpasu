import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded'
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/utils'
import { ProfileType } from '../../_modules/consts'
import { Action, Route as IndexRoute } from '../index'
import CreateProfileModal from './create-profile-modal'
import type { CreateKind, CreateSource } from './create-profile-schema'

export default function ImportButton() {
  const { type } = IndexRoute.useParams()

  const { action } = IndexRoute.useSearch()

  const navigate = IndexRoute.useNavigate()

  const isProxy = type === ProfileType.Profile

  const [open, setOpen] = useState(false)

  const [source, setSource] = useState<CreateSource>('remote')

  useEffect(() => {
    if (action !== Action.ImportLocalProfile) {
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
  }, [action])

  const handleOpenChange = (value: boolean) => {
    setOpen(value)

    if (!value && action !== null && action !== undefined) {
      navigate({ search: { action: null } })
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
      <Button
        variant="fab"
        icon
        onClick={() => {
          // transforms are usually written from a template
          setSource(isProxy ? 'remote' : 'local')
          setOpen(true)
        }}
      >
        <NoteStackAddRounded className="size-6" />
      </Button>

      <CreateProfileModal
        open={open}
        onOpenChange={handleOpenChange}
        kind={isProxy ? 'file' : (type as CreateKind)}
        source={source}
      />
    </div>
  )
}

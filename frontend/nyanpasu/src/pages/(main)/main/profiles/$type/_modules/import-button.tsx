import CloudDownloadRounded from '~icons/material-symbols/cloud-download-rounded'
import FileOpenRounded from '~icons/material-symbols/file-open-rounded'
import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded'
import { AnimatePresence } from 'framer-motion'
import { ComponentProps, useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollArea } from '@/components/ui/scroll-area'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
import { ProfileType } from '../../_modules/consts'
import { Route as IndexRoute } from '../index'
import ChainProfileImport from './chain-profile-import'
import LocalProfileButton from './local-profile-button'
import RemoteProfileButton from './remote-profile-button'

const SelectButton = ({
  className,
  label,
  ...props
}: ComponentProps<typeof Button> & {
  label?: string
}) => {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          className={cn(
            'flex size-10 items-center justify-center gap-2',
            'bg-primary-container dark:bg-surface-variant/30',
            className,
          )}
          variant="fab"
          icon
          {...props}
        />
      </TooltipTrigger>

      {label && (
        <TooltipContent side="left">
          <span>{label}</span>
        </TooltipContent>
      )}
    </Tooltip>
  )
}

const ProxyProfileImport = () => {
  const { isScrolling } = useScrollArea()

  const [open, setOpen] = useState(false)

  const handleToggle = () => {
    setOpen(!open)
  }

  // close the modal when scrolling
  useEffect(() => {
    if (isScrolling && open) {
      setOpen(false)
    }
  }, [isScrolling, open])

  return (
    <div className="relative">
      <Button className="z-10" variant="fab" icon onClick={handleToggle}>
        <NoteStackAddRounded className="size-6" />
      </Button>

      <AnimatePresence initial={false}>
        <div
          className={cn(
            'absolute flex w-full flex-col items-center gap-4',
            'top-0 scale-0 opacity-0 transition-[top,opacity,scale] duration-300 ease-in-out',
            open && '-top-28 scale-100 opacity-100',
          )}
        >
          <RemoteProfileButton>
            <SelectButton label={m.profile_import_remote_title()}>
              <CloudDownloadRounded />
            </SelectButton>
          </RemoteProfileButton>

          <LocalProfileButton>
            <SelectButton label={m.profile_import_local_title()}>
              <FileOpenRounded />
            </SelectButton>
          </LocalProfileButton>
        </div>
      </AnimatePresence>
    </div>
  )
}

export default function ImportButton() {
  const { type } = IndexRoute.useParams()

  const isProxy = type === ProfileType.Profile

  return (
    <div
      className={cn(
        'absolute',
        'right-4 transition-[top] duration-500',
        'top-[calc(100vh-40px-64px-72px)]',
        'sm:top-[calc(100vh-40px-48px-72px)]',
        'group-data-[scroll-direction=down]/profiles-content:top-full',
      )}
    >
      {isProxy ? <ProxyProfileImport /> : <ChainProfileImport />}
    </div>
  )
}

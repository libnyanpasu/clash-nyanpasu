import ArrowDropDownRounded from '~icons/material-symbols/arrow-drop-down-rounded'
import { cva } from 'class-variance-authority'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'

const logFileMenuTriggerVariants = cva(
  'text-on-surface-variant flex h-10 w-40 min-w-28 shrink items-center gap-1 rounded-full px-3 text-sm lg:w-56',
)

const logFileMenuItemVariants = cva(
  'h-auto min-h-12 rounded-xl [&>div]:min-w-0 [&>div]:break-all [&>span]:shrink-0',
)

export function LogFileMenu({
  file,
  files,
  onValueChange,
}: {
  file: string | null
  files: readonly { id: string; name: string }[]
  onValueChange: (file: string | null) => void
}) {
  const fileName = file
    ? (files.find((entry) => entry.id === file)?.name ?? file)
    : m.logs_current_file()

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          aria-label={m.logs_file_label()}
          title={fileName}
          className={logFileMenuTriggerVariants()}
        >
          <span className="min-w-0 flex-1 truncate text-left">{fileName}</span>
          <ArrowDropDownRounded aria-hidden className="size-5 shrink-0" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align="end"
        sideOffset={8}
        collisionPadding={8}
        className="w-80 max-w-[calc(100vw-1rem)] rounded-2xl p-1"
      >
        <DropdownMenuLabel className="text-on-surface-variant h-9 text-xs">
          {m.logs_file_label()}
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={file ?? 'current'}
          onValueChange={(value) =>
            onValueChange(value === 'current' ? null : value)
          }
        >
          <DropdownMenuRadioItem
            value="current"
            className={logFileMenuItemVariants()}
          >
            {m.logs_current_file()}
          </DropdownMenuRadioItem>
          {files.map((entry) => (
            <DropdownMenuRadioItem
              key={entry.id}
              value={entry.id}
              className={logFileMenuItemVariants()}
            >
              {entry.name}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

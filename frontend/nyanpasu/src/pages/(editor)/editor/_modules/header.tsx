import { ComponentProps } from 'react'
import { DefaultHeader, MacOSHeader } from '@/components/window/system-titlebar'
import WindowControl from '@/components/window/window-control'
import WindowTitle from '@/components/window/window-title'
import { m } from '@/paraglide/messages'
import { PlatformMacos } from '@nyanpasu/platform/react'
import { cn } from '@nyanpasu/utils'

const APP_NAME = 'Clash Nyanpasu'

/** The document being edited, marked while it has unsaved changes. */
const Title = ({ title, dirty }: { title?: string; dirty?: boolean }) => {
  return (
    <WindowTitle className="min-w-0">
      <div
        className="text-on-surface flex min-w-0 items-center gap-2 text-base text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {dirty && (
          <span
            className="bg-primary size-2 shrink-0 rounded-full"
            role="img"
            aria-label={m.editor_unsaved_changes()}
            data-slot="editor-title-unsaved"
            data-tauri-drag-region
          />
        )}

        {title && (
          <span
            className="truncate font-bold"
            data-slot="editor-title-name"
            data-tauri-drag-region
          >
            {title}
          </span>
        )}

        <span
          className={cn(
            'truncate',
            title ? 'text-on-surface/60 text-sm' : 'font-bold',
          )}
          data-slot="editor-title-app"
          data-tauri-drag-region
        >
          {APP_NAME}
        </span>
      </div>
    </WindowTitle>
  )
}

export default function Header({
  className,
  title,
  dirty,
  beforeClose,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose']
  /** The name of the document being edited. */
  title?: string
  dirty?: boolean
}) {
  return (
    <PlatformMacos
      fallback={
        <DefaultHeader
          className={className}
          beforeClose={beforeClose}
          {...props}
        >
          <Title title={title} dirty={dirty} />
        </DefaultHeader>
      }
    >
      <MacOSHeader className={className} {...props}>
        <Title title={title} dirty={dirty} />
      </MacOSHeader>
    </PlatformMacos>
  )
}

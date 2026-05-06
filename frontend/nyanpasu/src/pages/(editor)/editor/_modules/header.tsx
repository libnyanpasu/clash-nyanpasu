import { ComponentProps } from 'react'
import { DefaultHeader, MacOSHeader } from '@/components/window/system-titlebar'
import WindowControl from '@/components/window/window-control'
import WindowTitle from '@/components/window/window-title'
import { isMacOS } from '@/consts'

const DEFAULT_APP_NAME = 'Clash Nyanpasu - Editor'

const Title = ({ title }: { title: string }) => {
  return (
    <WindowTitle>
      <div
        className="text-on-surface text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {title}
      </div>
    </WindowTitle>
  )
}

export default function Header({
  className,
  title = DEFAULT_APP_NAME,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose']
  title?: string
}) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <Title title={title} />
    </MacOSHeader>
  ) : (
    <DefaultHeader className={className} {...props}>
      <Title title={title} />
    </DefaultHeader>
  )
}

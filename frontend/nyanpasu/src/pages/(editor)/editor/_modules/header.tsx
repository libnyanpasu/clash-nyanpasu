import { ComponentProps } from 'react'
import { DefaultHeader, MacOSHeader } from '@/components/window/system-titlebar'
import WindowControl from '@/components/window/window-control'
import WindowTitle from '@/components/window/window-title'
import { isMacOS } from '@/consts'

const APP_NAME = 'Clash Nyanpasu - Editor'

const Title = () => {
  return (
    <WindowTitle>
      <div
        className="text-on-surface text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {APP_NAME}
      </div>
    </WindowTitle>
  )
}

export default function Header({
  className,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose']
}) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <Title />
    </MacOSHeader>
  ) : (
    <DefaultHeader className={className} {...props}>
      <Title />
    </DefaultHeader>
  )
}

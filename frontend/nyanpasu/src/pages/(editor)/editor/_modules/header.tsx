import { ComponentProps } from 'react'
import WindowControl from '@/components/window/window-control'
import WindowHeader from '@/components/window/window-header'
import WindowTitle from '@/components/window/window-title'

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
  beforeClose,
}: {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose']
}) {
  return (
    <WindowHeader
      className="shrink-0 items-center justify-between px-3"
      data-slot="window-control"
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <Title />
      </div>

      <WindowControl hiddenAlwaysOnTop beforeClose={beforeClose} />
    </WindowHeader>
  )
}

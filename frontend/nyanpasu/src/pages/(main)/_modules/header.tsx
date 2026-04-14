import { ComponentProps } from 'react'
import {
  DefaultHeader,
  MacOSHeader,
  MacOSHeaderLeft,
} from '@/components/window/system-titlebar'
import WindowTitle from '@/components/window/window-title'
import { isMacOS } from '@/consts'
import HeaderMenu from './header-menu'

const APP_NAME = 'Clash Nyanpasu'

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

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <MacOSHeaderLeft>
        <HeaderMenu />
      </MacOSHeaderLeft>

      <Title />
    </MacOSHeader>
  ) : (
    <DefaultHeader className={className} {...props}>
      <Title />
      <HeaderMenu className="hidden md:flex" />
    </DefaultHeader>
  )
}

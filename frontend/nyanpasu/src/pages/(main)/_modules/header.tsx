import { ComponentProps } from 'react'
import {
  DefaultHeader,
  MacOSHeader,
  MacOSHeaderLeft,
} from '@/components/window/system-titlebar'
import WindowTitle from '@/components/window/window-title'
import { PlatformMacos } from '@nyanpasu/platform/react'
import HeaderMenu, { HeaderTitleMenu } from './header-menu'

const APP_NAME = 'Clash Nyanpasu'

const Title = ({ className }: { className?: string }) => {
  return (
    <WindowTitle className={className}>
      <div
        className="text-on-primary-container text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {APP_NAME}
      </div>
    </WindowTitle>
  )
}

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return (
    <PlatformMacos
      fallback={
        <DefaultHeader className={className} {...props}>
          <Title className="hidden md:flex" />
          <HeaderTitleMenu className="md:hidden" />
          <HeaderMenu className="hidden md:flex" />
        </DefaultHeader>
      }
    >
      <MacOSHeader className={className} {...props}>
        <MacOSHeaderLeft>
          <HeaderMenu />
        </MacOSHeaderLeft>

        <Title className="hidden md:flex" />
        <HeaderTitleMenu className="md:hidden" />
      </MacOSHeader>
    </PlatformMacos>
  )
}

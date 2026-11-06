import { DefaultHeader, MacOSHeader } from '@/components/window/system-titlebar'
import WindowTitle from '@/components/window/window-title'
import { m } from '@/paraglide/messages'
import { usePlatform } from '@nyanpasu/platform/react'
import { createFileRoute, Outlet } from '@tanstack/react-router'

export const Route = createFileRoute('/(inspector)/rejected-config')({
  component: RejectedConfigLayout,
})

function RejectedConfigLayout() {
  const Header =
    usePlatform().clientOs === 'macos' ? MacOSHeader : DefaultHeader

  return (
    <div
      data-slot="rejected-config-window"
      className="bg-background/30 flex h-dvh min-w-0 flex-col overflow-hidden"
    >
      <Header className="shrink-0">
        <WindowTitle className="min-w-0">
          <span
            className="text-on-surface min-w-0 truncate text-base font-bold"
            data-tauri-drag-region
          >
            {m.inspect_rejected_title()}
          </span>
        </WindowTitle>
      </Header>
      <Outlet />
    </div>
  )
}

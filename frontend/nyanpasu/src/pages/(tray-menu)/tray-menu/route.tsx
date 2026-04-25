import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands } from '@interface/ipc'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { ActionButton, ActionButtonSeparator } from './_modules/action-button'

export const Route = createFileRoute('/(tray-menu)/tray-menu')({
  component: RouteComponent,
})

const QuitActionButton = () => {
  const handleClick = useLockFn(async () => {
    await commands.quitApplication()
  })

  return <ActionButton onClick={handleClick}>{m.tray_menu_quit()}</ActionButton>
}

function RouteComponent() {
  return (
    <div
      className={cn(
        'h-dvh w-dvw overflow-hidden',
        'flex flex-col',
        'bg-background',
      )}
      data-slot="tray-menu-container"
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="min-h-0 flex-1">
        <AnimatedOutletPreset className="h-full" />
      </div>

      <ActionButtonSeparator />

      <div className="flex flex-col gap-2 p-2">
        <QuitActionButton />
      </div>
    </div>
  )
}

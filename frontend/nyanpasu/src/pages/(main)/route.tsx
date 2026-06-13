import ContextMenuProvider from '@/components/providers/context-menu-provider'
import NyanpasuUpdateProvider from '@/components/providers/nyanpasu-update-provider'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { cn } from '@nyanpasu/utils'
import packageJson from '@root/package.json'
import { createFileRoute } from '@tanstack/react-router'
import Header from './_modules/header'
import Navbar from './_modules/navbar'

export const Route = createFileRoute('/(main)')({
  component: RouteComponent,
})

const AppContent = () => {
  return (
    <AnimatedOutletPreset
      className={cn(
        'flex min-h-0 flex-1 flex-col overflow-hidden',
        '[&>div]:min-h-0 [&>div]:flex-1',
      )}
      data-slot="app-content"
    />
  )
}

function RouteComponent() {
  return (
    <NyanpasuUpdateProvider>
      <ContextMenuProvider>
        <div
          className={cn(
            'flex max-h-dvh min-h-dvh flex-col overflow-hidden',
            'bg-mixed-background',
          )}
          data-slot="app-root"
          data-app-version={packageJson.version}
        >
          <Header className="shrink-0" />

          <div
            className="flex min-h-0 flex-1 flex-col-reverse sm:flex-col"
            data-slot="app-content-container"
          >
            <Navbar className="shrink-0" />

            <AppContent />
          </div>
        </div>
      </ContextMenuProvider>
    </NyanpasuUpdateProvider>
  )
}

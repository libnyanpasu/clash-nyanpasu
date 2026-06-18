import ContextMenuProvider from '@/components/providers/context-menu-provider'
import NyanpasuUpdateProvider from '@/components/providers/nyanpasu-update-provider'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import useIsMobile from '@/hooks/use-is-moblie'
import { cn } from '@nyanpasu/utils'
import packageJson from '@root/package.json'
import { createFileRoute } from '@tanstack/react-router'
import Header from './_modules/header'
import { DefaultNavbar, MobileNavbar } from './_modules/navbar'

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
  const isMobile = useIsMobile()

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
            className="flex min-h-0 flex-1 flex-col"
            data-slot="app-content-container"
          >
            {!isMobile && (
              <div
                className={cn(
                  'flex h-12 shrink-0 items-center gap-2 px-3',
                  'dark:bg-on-primary bg-primary-container',
                )}
                data-slot="app-navbar"
              >
                <DefaultNavbar />
              </div>
            )}

            <AppContent />

            {isMobile && (
              <div
                className={cn(
                  'flex h-16 shrink-0 items-center gap-2 px-3',
                  'dark:bg-scrim bg-primary-container',
                  'justify-between',
                )}
                data-slot="app-navbar-mobile"
              >
                {isMobile ? <MobileNavbar /> : <DefaultNavbar />}
              </div>
            )}
          </div>
        </div>
      </ContextMenuProvider>
    </NyanpasuUpdateProvider>
  )
}

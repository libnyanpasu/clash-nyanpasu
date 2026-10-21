import { AppContentScrollArea } from '@nyanpasu/ui/scroll-area'
import { useIsMobile } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(main)/main/settings/')({
  component: RouteComponent,
})

function RouteComponent() {
  const isMobile = useIsMobile()

  if (!isMobile) {
    return null
  }

  return (
    <AppContentScrollArea
      className={cn(
        // Opaque, unlike the desktop sidebar: route transitions overlap pages.
        'dark:bg-mixed-background bg-[color-mix(in_srgb,var(--color-surface-variant)_50%,var(--color-surface))]',
        'z-50 w-full flex-1 [&>div>div]:block!',
      )}
      data-slot="settings-sidebar-scroll-area"
    >
      <SettingsNavigate />
    </AppContentScrollArea>
  )
}

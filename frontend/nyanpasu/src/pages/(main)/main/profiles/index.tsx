import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { createFileRoute } from '@tanstack/react-router'
import ProfilesNavigate from './_modules/profiles-navigate'

export const Route = createFileRoute('/(main)/main/profiles/')({
  component: RouteComponent,
})

function RouteComponent() {
  const isMobile = useIsMobile()

  if (!isMobile) {
    return null
  }

  return (
    <AppContentScrollArea
      className="bg-surface-variant/10"
      data-slot="profiles-sidebar-scroll-area"
    >
      <ProfilesNavigate />
    </AppContentScrollArea>
  )
}

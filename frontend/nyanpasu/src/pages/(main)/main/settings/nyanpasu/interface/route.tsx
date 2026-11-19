import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface',
)({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_user_interface_title() } },
})

function RouteComponent() {
  return <AnimatedOutletPreset className="flex flex-1 flex-col" />
}

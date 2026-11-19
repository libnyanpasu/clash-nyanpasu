import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.tray_menu_proxies() } },
})

function RouteComponent() {
  return <AnimatedOutletPreset />
}

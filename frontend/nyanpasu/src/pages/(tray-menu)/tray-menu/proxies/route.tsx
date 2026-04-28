import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies')({
  component: RouteComponent,
})

function RouteComponent() {
  return <AnimatedOutletPreset />
}

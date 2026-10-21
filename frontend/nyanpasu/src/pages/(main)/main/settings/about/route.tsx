import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
})

function RouteComponent() {
  return <AnimatedOutletPreset className="flex flex-1 flex-col" />
}

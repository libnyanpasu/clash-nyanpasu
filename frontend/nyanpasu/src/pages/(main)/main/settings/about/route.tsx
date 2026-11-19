import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_about() } },
})

function RouteComponent() {
  return <AnimatedOutletPreset className="flex flex-1 flex-col" />
}

import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(main)/main/settings/system/proxy')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_proxy() } },
})

function RouteComponent() {
  return <AnimatedOutletPreset className="flex flex-1 flex-col" />
}

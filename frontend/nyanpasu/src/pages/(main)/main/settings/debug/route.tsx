import { m } from '@/paraglide/messages'
import { createFileRoute, Outlet } from '@tanstack/react-router'
import DebugProvider from './_modules/debug-provider'

export const Route = createFileRoute('/(main)/main/settings/debug')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_debug() } },
})

function RouteComponent() {
  return (
    <DebugProvider>
      <Outlet />
    </DebugProvider>
  )
}

import { createFileRoute, Outlet } from '@tanstack/react-router'
import DebugProvider from './_modules/debug-provider'

export const Route = createFileRoute('/(main)/main/settings/debug')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <DebugProvider>
      <Outlet />
    </DebugProvider>
  )
}

import { createFileRoute } from '@tanstack/react-router'
import ProfilesHeader from './_modules/profiles-header'
import ProfilesList from './_modules/profiles-list'

export const Route = createFileRoute(
  '/(experimental)/experimental/profiles/$type/',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <ProfilesHeader />

      <ProfilesList className="p-4" />
    </>
  )
}

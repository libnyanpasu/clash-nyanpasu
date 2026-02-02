import { createFileRoute } from '@tanstack/react-router'
import { ProfileType } from '../_modules/consts'
import ImportButton from './_modules/import-button'
import ProfilesHeader from './_modules/profiles-header'
import ProfilesList from './_modules/profiles-list'

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { type } = Route.useParams()

  const allowImport = type === ProfileType.Profile

  return (
    <>
      {allowImport && <ProfilesHeader />}

      <ProfilesList className="p-4" />

      <ImportButton />
    </>
  )
}

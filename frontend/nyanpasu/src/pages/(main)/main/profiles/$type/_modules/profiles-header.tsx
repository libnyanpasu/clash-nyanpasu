import { PageTitle } from '@/components/page-title'
import { ProfileType } from '../../_modules/consts'
import ProfileQuickImport from '../../_modules/profile-quick-import'
import { Route as IndexRoute } from '../index'

export default function ProfilesHeader() {
  const { type } = IndexRoute.useParams()

  const isProfileType = type === ProfileType.Profile

  return (
    <>
      <PageTitle />

      {isProfileType && (
        <div className="px-4 pb-4">
          <ProfileQuickImport />
        </div>
      )}
    </>
  )
}

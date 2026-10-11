import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { ProfileType } from '../../_modules/consts'
import ProfileQuickImport from '../../_modules/profile-quick-import'
import { Route as IndexRoute } from '../index'

export default function ProfilesHeader() {
  const { type } = IndexRoute.useParams()

  const isProfileType = type === ProfileType.Profile

  const messages = {
    [ProfileType.Profile]: m.profile_profile_label(),
    [ProfileType.JavaScript]: m.profile_javascript_label(),
    [ProfileType.Lua]: m.profile_lua_label(),
    [ProfileType.Merge]: m.profile_merge_label(),
  } satisfies Record<ProfileType, string>

  return (
    <>
      <PageTitle backTo="/main/profiles">
        {messages[type as ProfileType]}
      </PageTitle>

      {isProfileType && (
        <div className="px-4 pb-4">
          <ProfileQuickImport />
        </div>
      )}
    </>
  )
}

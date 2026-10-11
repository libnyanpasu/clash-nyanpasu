import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { useIsMobile } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../../_modules/consts'
import ProfileQuickImport from '../../_modules/profile-quick-import'
import { Route as IndexRoute } from '../index'

const BackButton = () => {
  return (
    <Button icon className="flex items-center justify-center md:hidden" asChild>
      <Link to="/main/profiles">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export default function ProfilesHeader() {
  const { type } = IndexRoute.useParams()

  const isMobile = useIsMobile()

  const isProfileType = type === ProfileType.Profile

  const messages = {
    [ProfileType.Profile]: m.profile_profile_label(),
    [ProfileType.JavaScript]: m.profile_javascript_label(),
    [ProfileType.Lua]: m.profile_lua_label(),
    [ProfileType.Merge]: m.profile_merge_label(),
  } satisfies Record<ProfileType, string>

  return (
    <div
      className={cn(
        'flex flex-col items-stretch gap-2 p-4',
        'bg-mixed-background text-on-surface',
      )}
    >
      <div className="flex items-center gap-2">
        {isMobile && <BackButton />}
        <h1 className="text-2xl font-medium">
          {messages[type as ProfileType]}
        </h1>
      </div>

      {isProfileType && <ProfileQuickImport />}
    </div>
  )
}

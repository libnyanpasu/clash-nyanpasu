import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { Button } from '@/components/ui/button'
import useIsMobile from '@/hooks/use-is-moblie'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
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
        'flex items-center gap-2 p-4',
        'sticky top-0 z-10',
        'backdrop-blur-xl',
      )}
    >
      {isMobile && <BackButton />}

      {isProfileType ? (
        <ProfileQuickImport />
      ) : (
        <p className="text-lg font-bold">{messages[type as ProfileType]}</p>
      )}
    </div>
  )
}

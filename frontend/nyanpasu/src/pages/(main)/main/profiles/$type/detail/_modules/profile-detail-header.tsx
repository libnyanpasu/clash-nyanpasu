import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import { Button } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { useNavigate, useRouter } from '@tanstack/react-router'
import { PROFILE_TYPE_NAMES, ProfileType } from '../../../_modules/consts'
import ProfileNameEditor from './profile-name-editor'

/** The page header, which stays above the overview card while the page scrolls. */
export default function ProfileDetailHeader({
  type,
  profile,
}: {
  type: ProfileType
  profile: ProfileItem_Serialize
}) {
  const router = useRouter()

  const navigate = useNavigate()

  // Return to wherever the page was opened from; a page opened first in this
  // session has nothing to return to, so it goes to its list.
  const goBack = () => {
    if (router.history.location.state.__TSR_index > 0) {
      router.history.back()
      return
    }

    navigate({ to: '/main/profiles/$type', params: { type } })
  }

  return (
    <header
      data-slot="profile-detail-header"
      className="bg-mixed-background sticky top-0 z-10 flex h-16 items-center gap-4"
    >
      <Button
        icon
        className="flex shrink-0 items-center justify-center"
        aria-label={m.profile_detail_back({
          destination: PROFILE_TYPE_NAMES[type],
        })}
        onClick={goBack}
      >
        <ArrowBackIosNewRounded className="size-4" aria-hidden />
      </Button>

      <h1 className="w-0 min-w-0 flex-1">
        <TextMarquee className="text-lg font-bold">{profile.name}</TextMarquee>
      </h1>

      <ProfileNameEditor profile={profile} asChild>
        <Button
          icon
          className="shrink-0"
          aria-label={m.profile_name_editor_title()}
        >
          <EditSquareOutlineRounded className="size-4" aria-hidden />
        </Button>
      </ProfileNameEditor>
    </header>
  )
}

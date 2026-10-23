import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import PowerSettingsNewRounded from '~icons/material-symbols/power-settings-new-rounded'
import { useRef } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter } from '@nyanpasu/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { getRemoteSource, isConfigItem } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { ActiveGradient } from '../../../_modules/active-gradient'
import ProfileCardAction from '../../../_modules/profile-card-action'
import ProfileCardContentSummary from '../../../_modules/profile-card-content-summary'
import ProfileCardFeatures, {
  ProfileCardChip,
} from '../../../_modules/profile-card-features'
import ProfileCardSubscription, {
  ProfileCardTraffic,
  ProfileCardUpdate,
} from '../../../_modules/profile-card-subscription'
import {
  ProfileMoveDropdownMenu,
  useProfileMove,
} from '../../../_modules/profile-move-menu'
import { useActiveProfile } from '../../../_modules/use-active-profile'
import { hasMaterializedFile, isProxyProfile } from '../../../_modules/utils'
import ViewContent from '../../../_modules/view-content'
import { useShowInFileManager } from './use-show-in-file-manager'

/** The profile card of the list, as the top of the detail page. */
export default function ProfileOverviewCard({
  profile,
  profiles,
  isGlobal,
}: {
  profile: ProfileItem_Serialize
  profiles: ProfileItem_Serialize[]
  isGlobal: boolean
}) {
  const activeProfile = useActiveProfile(profile)

  const showInFileManager = useShowInFileManager(profile)
  const move = useProfileMove(profile.uid)

  const remote = getRemoteSource(profile)
  const hasFile = hasMaterializedFile(profile)

  const cardRef = useRef<HTMLDivElement>(null)

  return (
    <Card
      data-slot="profile-detail-overview"
      data-profile-uid={profile.uid}
      data-active={activeProfile.isActive || undefined}
      className={cn(
        'relative isolate flex min-w-0 flex-col justify-between',
        activeProfile.isActive && 'rounded-4xl',
      )}
      asChild
    >
      <div ref={cardRef}>
        {activeProfile.isActive && <ActiveGradient cardRef={cardRef} />}

        <CardContent className="relative gap-2 px-4 py-4">
          <ProfileCardFeatures
            profile={profile}
            profiles={profiles}
            isGlobal={isGlobal}
            leading={
              <>
                {activeProfile.isActive && (
                  <ProfileCardChip className="bg-primary/60 text-on-primary shrink-0">
                    {m.profile_is_active_label()}
                  </ProfileCardChip>
                )}

                {move.folder && (
                  <ProfileCardChip data-slot="profile-folder-chip">
                    <FolderRounded className="size-3.5" aria-hidden />
                    {move.folder.name}
                  </ProfileCardChip>
                )}
              </>
            }
          />

          {remote ? (
            <ProfileCardSubscription profile={profile} featured />
          ) : isConfigItem(profile) ? (
            <ProfileCardTraffic profile={profile} featured />
          ) : null}

          {remote && isConfigItem(profile) && (
            <ProfileCardContentSummary uid={profile.uid} />
          )}
        </CardContent>

        <CardFooter className="relative shrink-0 flex-row items-center justify-between gap-1">
          {isProxyProfile(profile) && !activeProfile.isActive ? (
            <Button
              variant="stroked"
              className="flex shrink-0 items-center gap-2 px-4 text-xs"
              disabled={activeProfile.isPending}
              aria-label={m.profile_active_title()}
              onClick={activeProfile.handleClick}
            >
              <PowerSettingsNewRounded aria-hidden />
              {m.profile_active_title()}
            </Button>
          ) : (
            <span />
          )}

          <div className="flex shrink-0 items-center gap-1">
            {remote && (
              <ProfileCardUpdate
                profile={profile}
                disabled={activeProfile.isPending}
              />
            )}

            {hasFile && (
              <ProfileCardAction label={m.profile_view_content_title()}>
                <ViewContent
                  profile={profile}
                  icon
                  className="size-9 sm:size-11"
                  disabled={activeProfile.isPending}
                  aria-label={m.profile_view_content_title()}
                >
                  <EditSquareOutlineRounded className="size-5" aria-hidden />
                </ViewContent>
              </ProfileCardAction>
            )}

            {(hasFile || isConfigItem(profile)) && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    icon
                    className="size-9 sm:size-11"
                    aria-label={m.profile_detail_more_actions()}
                  >
                    <MoreVertRounded className="size-5" aria-hidden />
                  </Button>
                </DropdownMenuTrigger>

                <DropdownMenuContent align="end">
                  {hasFile && (
                    <DropdownMenuItem onSelect={showInFileManager}>
                      <FolderOpenOutlineRounded
                        className="size-4"
                        aria-hidden
                      />
                      <span>{m.profile_detail_show_in_file_manager()}</span>
                    </DropdownMenuItem>
                  )}

                  {isConfigItem(profile) && (
                    <ProfileMoveDropdownMenu move={move} />
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </div>
        </CardFooter>
      </div>
    </Card>
  )
}

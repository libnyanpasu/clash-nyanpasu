import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DragClickRounded from '~icons/material-symbols/drag-click-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import InfoOutlineRounded from '~icons/material-symbols/info-outline-rounded'
import PowerSettingsNewRounded from '~icons/material-symbols/power-settings-new-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { memo, useRef } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import { LinearProgress } from '@nyanpasu/ui/progress'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { pointerIntersection } from '@dnd-kit/collision'
import { useSortable } from '@dnd-kit/react/sortable'
import { getRemoteSource, isConfigItem } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import { ActiveGradient } from './active-gradient'
import ProfileCardAction from './profile-card-action'
import ProfileCardFeatures, { ProfileCardChip } from './profile-card-features'
import ProfileCardSubscription, {
  ProfileCardTraffic,
  ProfileCardUpdate,
} from './profile-card-subscription'
import { ProfileMoveContextMenu, useProfileMove } from './profile-move-menu'
import { PROFILE_DRAG_TYPE } from './resolve-profile-drop'
import { useActiveProfile } from './use-active-profile'
import { useDeleteProfile } from './use-delete-profile'
import { hasMaterializedFile, isProxyProfile, profileTypeOf } from './utils'
import ViewContent from './view-content'

export const GridViewProfile = memo(function GridViewProfile({
  profile,
  profiles,
  group,
  index,
  isGlobal,
}: {
  profile: ProfileItem_Serialize
  profiles: ProfileItem_Serialize[]
  /** The sortable group the card moves in: its section, when it has one. */
  group?: string
  index: number
  isGlobal: boolean
}) {
  const activeProfile = useActiveProfile(profile)
  const deleteProfile = useDeleteProfile(profile)
  const move = useProfileMove(profile.uid)

  const remote = getRemoteSource(profile)
  const featured = activeProfile.isActive && isProxyProfile(profile)

  const isPending = activeProfile.isPending || deleteProfile.isPending

  const cardRef = useRef<HTMLDivElement>(null)

  // The whole card is the drag source; the pointer sensor ignores its buttons.
  // The active card is listed apart from the sections and stays put.
  const { isDragging } = useSortable({
    id: profile.uid,
    index,
    group,
    type: PROFILE_DRAG_TYPE,
    accept: [PROFILE_DRAG_TYPE],
    collisionDetector: pointerIntersection,
    element: cardRef,
    sensors: WHOLE_ITEM_DRAG_SENSORS,
    disabled: activeProfile.isActive,
  })

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card
          data-slot="profile-card"
          data-profile-uid={profile.uid}
          data-active={featured || undefined}
          data-dragging={isDragging || undefined}
          className={cn(
            'relative isolate flex min-w-0 cursor-default flex-col justify-between select-none',
            !activeProfile.isActive && 'active:cursor-grabbing',
            isProxyProfile(profile)
              ? featured
                ? 'order-first col-span-full h-72 rounded-4xl'
                : 'h-60'
              : 'min-h-40',
            isDragging && 'shadow-lg',
          )}
          asChild
        >
          <div ref={cardRef}>
            <AnimatePresence initial={false}>
              {isPending && (
                <motion.div
                  data-slot="profile-card-mask"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className={cn(
                    'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
                    'flex flex-col items-center justify-center gap-2',
                  )}
                >
                  <LinearProgress className="w-2/3 max-w-60" indeterminate />

                  <p className="text-on-surface-variant text-xs">
                    {m.profile_pending_mask_message()}
                  </p>
                </motion.div>
              )}
            </AnimatePresence>

            {activeProfile.isActive && <ActiveGradient cardRef={cardRef} />}

            <CardHeader
              className="relative flex flex-col items-start justify-between gap-2 px-4 pt-4 sm:flex-row sm:items-center"
              data-slot="profile-card-title"
            >
              <TextMarquee
                className={cn(
                  'w-full min-w-0 flex-1 sm:w-auto',
                  featured && 'text-xl sm:text-2xl',
                )}
              >
                {profile.name}
              </TextMarquee>

              {activeProfile.isActive && (
                <ProfileCardChip className="bg-primary/60 text-on-primary shrink-0">
                  {m.profile_is_active_label()}
                </ProfileCardChip>
              )}
            </CardHeader>

            <CardContent
              className={cn(
                'relative min-h-0 flex-1 overflow-hidden px-4',
                isProxyProfile(profile) && !featured
                  ? 'gap-1 py-2'
                  : 'gap-2 py-3',
              )}
            >
              <ProfileCardFeatures
                profile={profile}
                profiles={profiles}
                isGlobal={isGlobal}
                compact={isProxyProfile(profile) && !featured}
              />

              {remote ? (
                <ProfileCardSubscription
                  profile={profile}
                  featured={featured}
                />
              ) : isConfigItem(profile) ? (
                <ProfileCardTraffic profile={profile} featured={featured} />
              ) : null}
            </CardContent>

            <CardFooter className="relative shrink-0 flex-row items-center justify-between gap-1">
              {isProxyProfile(profile) && !activeProfile.isActive ? (
                <Button
                  variant="stroked"
                  className="flex shrink-0 items-center gap-2 px-4 text-xs"
                  disabled={isPending}
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
                  <ProfileCardUpdate profile={profile} disabled={isPending} />
                )}

                {hasMaterializedFile(profile) && (
                  <ProfileCardAction label={m.profile_view_content_title()}>
                    <ViewContent
                      profile={profile}
                      icon
                      className="size-9 sm:size-11"
                      disabled={isPending}
                      aria-label={m.profile_view_content_title()}
                    >
                      <EditSquareOutlineRounded
                        className="size-5"
                        aria-hidden
                      />
                    </ViewContent>
                  </ProfileCardAction>
                )}

                <ProfileCardAction label={m.profile_view_details_title()}>
                  <Button icon className="size-9 sm:size-11" asChild>
                    <Link
                      to="/main/profiles/$type/detail/$uid"
                      params={{
                        type: profileTypeOf(profile),
                        uid: profile.uid,
                      }}
                      aria-label={m.profile_view_details_title()}
                    >
                      <InfoOutlineRounded className="size-5" aria-hidden />
                    </Link>
                  </Button>
                </ProfileCardAction>
              </div>
            </CardFooter>
          </div>
        </Card>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        {isProxyProfile(profile) && (
          <ContextMenuItem
            disabled={isPending}
            onClick={activeProfile.handleClick}
          >
            <DragClickRounded className="size-4" />
            <span>{m.profile_active_title()}</span>
          </ContextMenuItem>
        )}

        {isConfigItem(profile) && (
          <ProfileMoveContextMenu move={move} disabled={isPending} />
        )}

        <ContextMenuItem
          disabled={isPending}
          onClick={deleteProfile.handleClick}
        >
          <DeleteForeverOutlineRounded className="size-4" />
          <span>{m.profile_delete_title()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
})

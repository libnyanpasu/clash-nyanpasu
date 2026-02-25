import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DragClickRounded from '~icons/material-symbols/drag-click-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import BorderBeam from '@/components/ui/border-beam'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { LinearProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { Profile, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'
import { PROFILE_TYPES } from '../../_modules/consts'
import { useActiveProfile } from '../detail/_modules/active-button'
import { useDeleteProfile } from '../detail/_modules/delete-profile'
import { Route as IndexRoute } from '../index'

const GridViewProfile = ({ profile }: { profile: Profile }) => {
  const { type } = IndexRoute.useParams()

  const activeProfile = useActiveProfile(profile)
  const deleteProfile = useDeleteProfile(profile)

  const isPending = activeProfile.isPending || deleteProfile.isPending

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card data-slot="profile-card" className="relative">
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

          {activeProfile.isActive && <BorderBeam size={200} />}

          <CardHeader data-slot="profile-card-title">
            <TextMarquee>{profile.name}</TextMarquee>
          </CardHeader>

          <CardContent>
            <div data-slot="profile-card-type">{profile.type}</div>
          </CardContent>

          <CardFooter>
            <Button className="flex items-center justify-center" asChild>
              <Link
                to="/main/profiles/$type/detail/$uid"
                params={{
                  type,
                  uid: profile.uid,
                }}
              >
                {m.profile_view_details_title()}
              </Link>
            </Button>
          </CardFooter>
        </Card>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem
          disabled={isPending}
          onClick={activeProfile.handleClick}
        >
          <DragClickRounded className="size-4" />
          <span>{m.profile_active_title()}</span>
        </ContextMenuItem>

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
}

const EmptyList = () => {
  return (
    <div
      className={cn(
        'mb-4 flex h-16 items-center justify-center text-center text-sm',
        'text-on-surface-variant',
        'dark:text-on-surface-variant-dark',
        'min-h-[calc(100vh-40px-64px-80px)]',
      )}
    >
      {m.profile_empty_list_message()}
    </div>
  )
}

const NoMoreProfiles = () => {
  return (
    <div className="mb-4 flex h-16 items-center justify-center text-center text-sm text-gray-500">
      {m.profile_no_more_profiles()}
    </div>
  )
}

export default function ProfilesList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { type } = IndexRoute.useParams()

  const {
    query: { data: profiles },
  } = useProfile()

  // Type guard: restrict type to the allowed PROFILE_TYPES keys
  const allowedTypes = PROFILE_TYPES[type as keyof typeof PROFILE_TYPES]

  // Filter by allowed types, fallback to no filtering if not found
  const filteredProfiles = profiles?.items?.filter(
    (profile) =>
      Array.isArray(allowedTypes) &&
      allowedTypes.some((t) => {
        // Check if type matches
        if (t.type !== profile.type) {
          return false
        }

        // If script_type is specified in allowedTypes, also check profile's script_type
        if ('script_type' in t && t.script_type !== undefined) {
          return (
            profile.type === 'script' &&
            'script_type' in profile &&
            profile.script_type === t.script_type
          )
        }

        // If script_type is not specified, type match is sufficient
        return true
      }),
  )

  // If no profiles are found, show the empty list message
  if (!filteredProfiles || filteredProfiles.length === 0) {
    return <EmptyList />
  }

  return (
    <>
      <div
        className={cn(
          'flex flex-col gap-4',
          'min-h-[calc(100vh-40px-64px)]',
          'sm:min-h-[calc(100vh-40px-48px)]',
        )}
      >
        <div
          className={cn(
            'grid gap-2',
            'md:grid-cols-2',
            'lg:grid-cols-3',
            'dxl:grid-cols-4',
            className,
          )}
          data-slot="profiles-navigate"
          {...props}
        >
          {filteredProfiles.map((profile) => (
            <GridViewProfile key={profile.uid} profile={profile} />
          ))}
        </div>

        <div className="flex-1" />
      </div>

      <NoMoreProfiles />
    </>
  )
}

import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DragClickRounded from '~icons/material-symbols/drag-click-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { LinearProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { hexFromArgb } from '@material/material-color-utilities'
import { Profile, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { MeshGradient } from '@paper-design/shaders-react'
import { Link } from '@tanstack/react-router'
import { PROFILE_TYPES } from '../../_modules/consts'
import { useActiveProfile } from '../detail/_modules/active-button'
import { useDeleteProfile } from '../detail/_modules/delete-profile'
import { Route as IndexRoute } from '../index'

const Chip = ({ children, className, ...props }: ComponentProps<'span'>) => {
  return (
    <span
      className={cn(
        'bg-primary-container rounded-full px-3 py-1 text-xs font-bold whitespace-nowrap',
        className,
      )}
      {...props}
    >
      {children}
    </span>
  )
}

const GridViewProfile = ({ profile }: { profile: Profile }) => {
  const { type } = IndexRoute.useParams()

  const activeProfile = useActiveProfile(profile)
  const deleteProfile = useDeleteProfile(profile)

  const isPending = activeProfile.isPending || deleteProfile.isPending

  const isRemote = profile.type === 'remote'

  const { themePalette } = useExperimentalThemeContext()

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card
          data-slot="profile-card"
          className="relative flex flex-col justify-between"
        >
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

          {activeProfile.isActive && (
            <MeshGradient
              className="absolute inset-0 size-full opacity-30"
              colors={Object.values(themePalette.schemes.light).map((color) =>
                hexFromArgb(color),
              )}
              distortion={0.5}
              swirl={0.1}
              grainMixer={0}
              grainOverlay={0}
              speed={1 / 3}
            />
          )}

          <CardHeader
            className="flex items-center justify-between gap-2"
            data-slot="profile-card-title"
          >
            <TextMarquee className="z-10 min-w-0 flex-1">
              {profile.name}
            </TextMarquee>

            {activeProfile.isActive && (
              <Chip className="shrink-0">{m.profile_is_active_label()}</Chip>
            )}
          </CardHeader>

          <CardContent>
            <div className="z-10" data-slot="profile-card-type">
              {isRemote ? (
                <Chip>{m.profile_remote_label()}</Chip>
              ) : (
                <Chip>{m.profile_local_label()}</Chip>
              )}
            </div>
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

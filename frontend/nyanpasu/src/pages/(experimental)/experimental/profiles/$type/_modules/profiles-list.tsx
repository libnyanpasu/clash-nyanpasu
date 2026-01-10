import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import { Profile, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link, useParams } from '@tanstack/react-router'
import { PROFILE_TYPES } from '../../_modules/consts'

const GridViewProfile = ({ profile }: { profile: Profile }) => {
  const { type } = useParams({
    from: '/(experimental)/experimental/profiles/$type/',
  })

  return (
    <Card data-slot="profile-card">
      <CardHeader data-slot="profile-card-title">{profile.name}</CardHeader>

      <CardContent>
        <div data-slot="profile-card-type">{profile.type}</div>
      </CardContent>

      <CardFooter>
        <Button className="flex items-center justify-center" asChild>
          <Link
            to="/experimental/profiles/$type/detail/$uid"
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
  const { type } = useParams({
    from: '/(experimental)/experimental/profiles/$type/',
  })

  const {
    query: { data: profiles },
  } = useProfile()

  // Type guard: restrict type to the allowed PROFILE_TYPES keys
  const allowedTypes = PROFILE_TYPES[type as keyof typeof PROFILE_TYPES]

  // Filter by allowed types, fallback to no filtering if not found
  const filteredProfiles = profiles?.items?.filter(
    (profile) =>
      Array.isArray(allowedTypes) &&
      allowedTypes.some((t) => t.type === profile.type),
  )

  // If no profiles are found, show the empty list message
  if (!filteredProfiles || filteredProfiles.length === 0) {
    return <EmptyList />
  }

  return (
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

      <NoMoreProfiles />
    </div>
  )
}

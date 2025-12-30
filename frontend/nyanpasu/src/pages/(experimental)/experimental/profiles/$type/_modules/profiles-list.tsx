import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Profile, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link, useParams } from '@tanstack/react-router'

const GridViewProfile = ({ profile }: { profile: Profile }) => {
  const { type } = useParams({
    from: '/(experimental)/experimental/profiles/$type/',
  })

  return (
    <CardContent>
      <div className="text-lg font-bold" data-slot="profile-card-title">
        {profile.name}
      </div>

      <div data-slot="profile-card-type">{profile.type}</div>

      <div className="flex items-center justify-end">
        <Button asChild>
          <Link
            className="flex items-center justify-center"
            to="/experimental/profiles/$type/detail/$uid"
            params={{
              type,
              uid: profile.uid,
            }}
          >
            View
          </Link>
        </Button>
      </div>
    </CardContent>
  )
}

const Profiles = () => {
  const {
    query: { data: profiles },
  } = useProfile()

  return profiles?.items?.map((profile) => (
    <Card key={profile.uid} data-slot="profile-card">
      <GridViewProfile profile={profile} />
    </Card>
  ))
}

export default function ProfilesList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  return (
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
      <Profiles />
    </div>
  )
}

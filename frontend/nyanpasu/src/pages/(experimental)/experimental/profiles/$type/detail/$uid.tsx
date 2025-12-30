import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import { Button } from '@/components/ui/button'
import { useProfile } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import ActionCard from './_modules/action-card'
import DetialHeader from './_modules/detial-header'
import ProfileNameEditor from './_modules/profile-name-editor'
import { SubscriptionCard } from './_modules/subscription-card'

export const Route = createFileRoute(
  '/(experimental)/experimental/profiles/$type/detail/$uid',
)({
  component: RouteComponent,
})

function RouteComponent() {
  const { uid } = Route.useParams()

  const { query } = useProfile()

  const currentProfile = query.data?.items?.find((item) => item.uid === uid)

  // TODO: better error handling
  if (!currentProfile) {
    return null
  }

  const isRemoteProfile = currentProfile.type === 'remote'

  return (
    <>
      <DetialHeader>
        <div className="flex items-center gap-2">
          <div className="text-lg font-bold">{currentProfile.name}</div>

          <ProfileNameEditor profile={currentProfile} asChild>
            <Button icon>
              <EditSquareOutlineRounded className="size-4" />
            </Button>
          </ProfileNameEditor>
        </div>
      </DetialHeader>

      <div className="columns-1 gap-4 space-y-4 p-4 md:columns-2">
        {isRemoteProfile && <SubscriptionCard profile={currentProfile} />}

        <ActionCard profile={currentProfile} />
      </div>
    </>
  )
}

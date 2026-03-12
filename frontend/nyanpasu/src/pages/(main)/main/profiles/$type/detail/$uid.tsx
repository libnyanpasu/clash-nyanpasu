import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import { Button } from '@/components/ui/button'
import TextMarquee from '@/components/ui/text-marquee'
import { useProfile } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { isProxyProfile } from '../_modules/utils'
import ActionCard from './_modules/action-card'
import ChianEditorCard from './_modules/chian-editor-card'
import DetialHeader from './_modules/detial-header'
import ProfileNameEditor from './_modules/profile-name-editor'
import { SubscriptionCard } from './_modules/subscription-card'

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    component: RouteComponent,
  },
)

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
        <TextMarquee className="w-0 min-w-0 flex-1 text-lg font-bold">
          {currentProfile.name}
        </TextMarquee>

        <ProfileNameEditor profile={currentProfile} asChild>
          <Button icon className="shrink-0">
            <EditSquareOutlineRounded className="size-4" />
          </Button>
        </ProfileNameEditor>
      </DetialHeader>

      <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
        {isRemoteProfile && <SubscriptionCard profile={currentProfile} />}

        <ActionCard profile={currentProfile} />

        {isProxyProfile(currentProfile) && (
          <ChianEditorCard profile={currentProfile} />
        )}
      </div>
    </>
  )
}

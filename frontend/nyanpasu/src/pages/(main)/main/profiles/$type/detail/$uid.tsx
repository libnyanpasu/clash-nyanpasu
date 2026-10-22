import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { isRemoteItem, useProfile } from '@nyanpasu/query'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { PROFILE_TYPE_NAMES, ProfileType } from '../../_modules/consts'
import { isChainProfile, isProxyProfile } from '../../_modules/utils'
import DeleteSection from './_modules/delete-section'
import FileSection from './_modules/file-section'
import ProfileDetailHeader from './_modules/profile-detail-header'
import ProfileDetailSkeleton from './_modules/profile-detail-skeleton'
import ProfileOverviewCard from './_modules/profile-overview-card'
import SubscriptionSection from './_modules/subscription-section'
import SyncHistorySection from './_modules/sync-history-section'
import TransformChainSection from './_modules/transform-chain-section'
import UsedBySection from './_modules/used-by-section'

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    component: RouteComponent,
  },
)

function RouteComponent() {
  const { uid, type } = Route.useParams()

  const navigate = useNavigate()

  const { query } = useProfile()

  const profiles = query.data?.items ?? []

  const currentProfile = profiles.find((item) => item.uid === uid)

  const backToList = () =>
    navigate({ to: '/main/profiles/$type', params: { type } })

  return (
    <div
      data-slot="profile-detail"
      className="mx-auto flex w-full max-w-3xl flex-col gap-4 px-4 pb-8"
    >
      {query.isPending ? (
        <ProfileDetailSkeleton />
      ) : !currentProfile ? (
        <EmptyState className="flex-1" message={m.profile_detail_not_found()}>
          <Button variant="stroked" asChild>
            <Link to="/main/profiles/$type" params={{ type }}>
              {m.profile_detail_back({
                destination: PROFILE_TYPE_NAMES[type as ProfileType],
              })}
            </Link>
          </Button>
        </EmptyState>
      ) : (
        <>
          <ProfileDetailHeader
            type={type as ProfileType}
            profile={currentProfile}
          />

          <ProfileOverviewCard
            profile={currentProfile}
            profiles={profiles}
            isGlobal={!!query.data?.global_transforms?.includes(uid)}
          />

          {isChainProfile(currentProfile) && (
            <UsedBySection
              uid={currentProfile.uid}
              profiles={profiles}
              globalTransforms={query.data?.global_transforms ?? []}
            />
          )}

          {isRemoteItem(currentProfile) && (
            <SubscriptionSection profile={currentProfile} />
          )}

          {isProxyProfile(currentProfile) && (
            <TransformChainSection profile={currentProfile} />
          )}

          {isRemoteItem(currentProfile) && (
            <SyncHistorySection uid={currentProfile.uid} />
          )}

          <FileSection profile={currentProfile} />

          <DeleteSection profile={currentProfile} onDeleted={backToList} />
        </>
      )}
    </div>
  )
}

import { AnimatePresence } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { type Breakpoint } from '@nyanpasu/constants'
import { useContainerBreakpointValue } from '@nyanpasu/hooks'
import { isRemoteItem, useProfile } from '@nyanpasu/query'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { PROFILE_TYPE_NAMES, ProfileType } from '../../_modules/consts'
import { isChainProfile, isProxyProfile } from '../../_modules/utils'
import DeleteSection from './_modules/delete-section'
import {
  DETAIL_PANELS,
  DetailPanel,
  DetailPanelSlot,
  type DetailPanelKind,
  type DetailPanelLayout,
} from './_modules/detail-panel'
import FileSection from './_modules/file-section'
import PanelEntries from './_modules/panel-entries'
import ProfileDetailHeader from './_modules/profile-detail-header'
import ProfileDetailSkeleton from './_modules/profile-detail-skeleton'
import ProfileOverviewCard from './_modules/profile-overview-card'
import SubscriptionSection from './_modules/subscription-section'
import SyncHistory from './_modules/sync-history'
import TransformsPanel from './_modules/transforms-panel'
import UsedBySection from './_modules/used-by-section'

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    component: RouteComponent,
    // Which panel is open is part of the address, so history and reload keep it.
    validateSearch: z.object({
      panel: z.enum(DETAIL_PANELS).optional().catch(undefined),
    }),
    // The detail column and the panel scroll on their own.
    staticData: { layout: 'fill' },
  },
)

// The detail column and the panel fit side by side from this container width
// on (the shared `md` breakpoint); below it the panel takes the column's place.
const PANEL_LAYOUTS: Partial<Record<Breakpoint, DetailPanelLayout>> = {
  xs: 'narrow',
  md: 'wide',
}

function RouteComponent() {
  const { uid, type } = Route.useParams()

  const { panel } = Route.useSearch()

  const navigate = useNavigate({ from: Route.fullPath })

  const { query } = useProfile()

  const profiles = query.data?.items ?? []

  const currentProfile = profiles.find((item) => item.uid === uid)

  const workspaceRef = useRef<HTMLDivElement>(null)

  const layout = useContainerBreakpointValue(workspaceRef, PANEL_LAYOUTS)

  // A panel the profile has no use for, such as the sync history of a local
  // file, is the same as no panel.
  const openPanel: DetailPanelKind | undefined =
    (panel === 'transforms' &&
      currentProfile &&
      isProxyProfile(currentProfile)) ||
    (panel === 'sync' && currentProfile && isRemoteItem(currentProfile))
      ? panel
      : undefined

  // The row of a panel that has just closed takes the focus back. The ref
  // still holds the previous panel while the closing render runs.
  const shownPanelRef = useRef(openPanel)

  const closedPanel =
    openPanel === undefined ? shownPanelRef.current : undefined

  useEffect(() => {
    shownPanelRef.current = openPanel
  }, [openPanel])

  // Every opening is a panel of its own, so one that is still sliding out is
  // never brought back, draft included, by opening it again.
  const [opening, setOpening] = useState({ count: 0, open: !!openPanel })

  if (opening.open !== !!openPanel) {
    setOpening({
      count: openPanel ? opening.count + 1 : opening.count,
      open: !!openPanel,
    })
  }

  const openPanelOf = (next: DetailPanelKind) =>
    navigate({ search: { panel: next } })

  const closePanel = () => navigate({ search: {} })

  const backToList = () =>
    navigate({ to: '/main/profiles/$type', params: { type } })

  return (
    <div
      ref={workspaceRef}
      data-slot="profile-detail-workspace"
      className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden"
    >
      {/* The panel covers the column when there is no room for both. */}
      <ScrollArea
        data-slot="profile-detail-scroll-area"
        className="min-h-0 min-w-0 flex-1"
        inert={!!openPanel && layout === 'narrow'}
      >
        <div
          data-slot="profile-detail"
          className="mx-auto flex w-full max-w-3xl flex-col gap-4 px-4 pb-8"
        >
          {query.isPending ? (
            <ProfileDetailSkeleton />
          ) : !currentProfile ? (
            <EmptyState
              className="flex-1"
              message={m.profile_detail_not_found()}
            >
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

              <PanelEntries
                profile={currentProfile}
                panel={openPanel}
                restoreFocus={closedPanel}
                onOpen={openPanelOf}
              />

              <FileSection profile={currentProfile} />

              <DeleteSection profile={currentProfile} onDeleted={backToList} />
            </>
          )}
        </div>
      </ScrollArea>

      <AnimatePresence initial={false}>
        {currentProfile && openPanel && (
          <DetailPanelSlot key={opening.count} layout={layout}>
            {openPanel === 'transforms' && isProxyProfile(currentProfile) && (
              <TransformsPanel
                key={currentProfile.uid}
                profile={currentProfile}
                layout={layout}
                onClose={closePanel}
              />
            )}

            {openPanel === 'sync' && (
              <DetailPanel
                key={currentProfile.uid}
                title={m.profile_sync_history_title()}
                profileName={currentProfile.name}
                layout={layout}
                onClose={closePanel}
              >
                <SyncHistory uid={currentProfile.uid} />
              </DetailPanel>
            )}
          </DetailPanelSlot>
        )}
      </AnimatePresence>
    </div>
  )
}

import { type ReactNode } from 'react'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import SyncConflictsCard from './_modules/sync-conflicts-card'
import SyncProviderCard from './_modules/sync-provider-card'
import SyncSnapshotsCard from './_modules/sync-snapshots-card'
import SyncStatusCard from './_modules/sync-status-card'

export const Route = createFileRoute('/(main)/main/settings/sync/')({
  component: RouteComponent,
})

function SyncMasonryItem({ children }: { children: ReactNode }) {
  return (
    <div className="break-inside-avoid pb-4" data-slot="sync-masonry-item">
      {children}
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_sync()}</PageTitle>

      <ReadOnlyScope scopes={['settings']}>
        <div className="space-y-4 px-4 pb-4">
          <div
            className="-mb-4 columns-1 gap-x-4 md:columns-2"
            data-slot="sync-masonry"
          >
            <SyncMasonryItem>
              <SyncStatusCard />
            </SyncMasonryItem>

            <SyncMasonryItem>
              <SyncProviderCard />
            </SyncMasonryItem>

            <SyncMasonryItem>
              <SyncConflictsCard />
            </SyncMasonryItem>

            <SyncMasonryItem>
              <SyncSnapshotsCard />
            </SyncMasonryItem>
          </div>
        </div>
      </ReadOnlyScope>
    </>
  )
}

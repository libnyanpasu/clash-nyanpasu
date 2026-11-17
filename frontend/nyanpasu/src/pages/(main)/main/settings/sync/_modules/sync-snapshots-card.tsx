import HistoryRounded from '~icons/material-symbols/history-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { PlatformDesktop } from '@nyanpasu/platform/react'
import { useSync } from '@nyanpasu/query'
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card'
import SyncCardHeader from './sync-card-header'

export default function SyncSnapshotsCard() {
  return (
    <PlatformDesktop
      fallback={
        <SettingsCard data-slot="sync-snapshots-card">
          <SyncCardHeader icon={HistoryRounded}>
            {m.settings_sync_snapshots_title()}
          </SyncCardHeader>
          <SettingsCardContent className="pt-2">
            <p className="text-on-surface-variant text-sm">
              {m.settings_sync_desktop_only()}
            </p>
          </SettingsCardContent>
        </SettingsCard>
      }
    >
      <SyncSnapshotsControls />
    </PlatformDesktop>
  )
}

function SyncSnapshotsControls() {
  const sync = useSync()
  const snapshots = sync.snapshots.data ?? []

  const restore = async (snapshotId: string) => {
    try {
      await sync.restoreSnapshot.mutateAsync(snapshotId)
      message(m.settings_sync_restored())
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <SettingsCard data-slot="sync-snapshots-card">
      <SyncCardHeader icon={HistoryRounded}>
        {m.settings_sync_snapshots_title()}
      </SyncCardHeader>
      <SettingsCardContent className="gap-2 pt-2">
        {snapshots.length === 0 ? (
          <p className="text-on-surface-variant text-sm">
            {m.settings_sync_no_snapshots()}
          </p>
        ) : (
          snapshots.map((snapshot) => (
            <div
              key={snapshot.id}
              className="flex items-center justify-between gap-3 text-sm"
            >
              <div className="flex min-w-0 flex-col">
                <span className="truncate">{snapshot.id}</span>
                <span className="text-on-surface-variant text-xs">
                  {snapshot.created_at} · {snapshot.items.length}
                </span>
              </div>
              <Button
                variant="stroked"
                disabled={sync.restoreSnapshot.isPending}
                onClick={() => restore(snapshot.id)}
              >
                {m.settings_sync_restore()}
              </Button>
            </div>
          ))
        )}
      </SettingsCardContent>
    </SettingsCard>
  )
}

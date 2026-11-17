import MergeRounded from '~icons/material-symbols/merge-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSync } from '@nyanpasu/query'
import type { ConflictChoice } from '@nyanpasu/rpc/types'
import { PlatformDesktop } from '@nyanpasu/platform/react'
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card'
import SyncCardHeader from './sync-card-header'

const CHOICES: ConflictChoice[] = ['local', 'remote', 'keep_both']

export default function SyncConflictsCard() {
  return (
    <PlatformDesktop
      fallback={
        <SettingsCard data-slot="sync-conflicts-card">
          <SyncCardHeader icon={MergeRounded}>
            {m.settings_sync_conflicts_title()}
          </SyncCardHeader>
          <SettingsCardContent className="pt-2">
            <p className="text-on-surface-variant text-sm">
              {m.settings_sync_desktop_only()}
            </p>
          </SettingsCardContent>
        </SettingsCard>
      }
    >
      <SyncConflictsControls />
    </PlatformDesktop>
  )
}

function SyncConflictsControls() {
  const sync = useSync()
  const conflicts = sync.conflicts.data ?? []

  const resolve = async (conflictId: string, choice: ConflictChoice) => {
    try {
      await sync.resolveConflict.mutateAsync({ conflictId, choice })
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <SettingsCard data-slot="sync-conflicts-card">
      <SyncCardHeader icon={MergeRounded}>
        {m.settings_sync_conflicts_title()}
      </SyncCardHeader>
      <SettingsCardContent className="gap-3 pt-2">
        {conflicts.length === 0 ? (
          <p className="text-on-surface-variant text-sm">
            {m.settings_sync_no_conflicts()}
          </p>
        ) : (
          conflicts.map((conflict) => (
            <div
              key={conflict.id}
              className="flex flex-col gap-2 rounded-xl border p-3"
              data-slot="sync-conflict"
            >
              <div className="flex flex-col text-sm">
                <span className="font-medium">{conflict.item}</span>
                <span className="text-on-surface-variant">{conflict.unit}</span>
              </div>
              <div className="grid grid-cols-2 gap-2 text-xs">
                <pre className="overflow-x-auto rounded-lg bg-surface-variant/50 p-2">
                  {conflict.local ?? m.settings_sync_absent()}
                </pre>
                <pre className="overflow-x-auto rounded-lg bg-surface-variant/50 p-2">
                  {conflict.remote ?? m.settings_sync_absent()}
                </pre>
              </div>
              <div className="flex flex-wrap justify-end gap-2">
                {CHOICES.map((choice) => (
                  <Button
                    key={choice}
                    variant="stroked"
                    disabled={sync.resolveConflict.isPending}
                    onClick={() => resolve(conflict.id, choice)}
                  >
                    {m.settings_sync_resolution({ choice })}
                  </Button>
                ))}
              </div>
            </div>
          ))
        )}
      </SettingsCardContent>
    </SettingsCard>
  )
}

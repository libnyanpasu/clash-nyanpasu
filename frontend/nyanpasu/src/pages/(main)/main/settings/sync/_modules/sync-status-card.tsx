import CloudSyncRounded from '~icons/material-symbols/cloud-sync-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { PlatformDesktop } from '@nyanpasu/platform/react'
import { useSync } from '@nyanpasu/query'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../../_modules/settings-card'
import SyncCardHeader from './sync-card-header'

export default function SyncStatusCard() {
  return (
    <PlatformDesktop
      fallback={
        <SettingsCard data-slot="sync-status-card">
          <SyncCardHeader icon={CloudSyncRounded}>
            {m.settings_sync_status_title()}
          </SyncCardHeader>
          <SettingsCardContent className="pt-2">
            <p className="text-on-surface-variant text-sm">
              {m.settings_sync_desktop_only()}
            </p>
          </SettingsCardContent>
        </SettingsCard>
      }
    >
      <SyncStatusControls />
    </PlatformDesktop>
  )
}

function SyncStatusControls() {
  const sync = useSync()
  const status = sync.status.data
  const settings = sync.settings.data?.settings
  const availability = status?.availability ?? 'disabled'
  const running = status != null && status.phase !== 'idle'

  const save = async (operation: Promise<unknown>) => {
    try {
      await operation
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  }

  const lastResult = status?.last_result
  const action = settings?.enabled ? sync.syncNow : sync.enable
  const supported = availability !== 'unsupported'

  return (
    <SettingsCard data-slot="sync-status-card">
      <SyncCardHeader icon={CloudSyncRounded}>
        {m.settings_sync_status_title()}
      </SyncCardHeader>
      <SettingsCardContent className="gap-2 pt-2 text-sm">
        {availability === 'unsupported' ? (
          <p className="text-on-surface-variant">
            {m.settings_sync_unavailable()}
          </p>
        ) : (
          <>
            <Row
              label={m.settings_sync_phase()}
              value={m.settings_sync_phase_value({
                phase: status?.phase ?? 'idle',
              })}
            />
            <Row
              label={m.settings_sync_last_result()}
              value={lastResult ?? m.settings_sync_never()}
            />
            <Row
              label={m.settings_sync_conflicts()}
              value={String(status?.open_conflicts ?? 0)}
            />
            {status?.head ? (
              <Row
                label={m.settings_sync_head_device()}
                value={`${status.head.device_name} · ${status.head.generation}`}
              />
            ) : null}
            {status?.last_error ? (
              <Row
                label={m.settings_sync_last_error()}
                value={status.last_error.kind}
              />
            ) : null}
            {status?.diverged ? (
              <p className="text-on-surface-variant">
                {m.settings_sync_diverged()}
              </p>
            ) : null}
          </>
        )}
      </SettingsCardContent>
      {supported ? (
        <SettingsCardFooter className="justify-end gap-2">
          {settings?.enabled ? (
            <Button
              variant="stroked"
              disabled={sync.disable.isPending}
              onClick={() => save(sync.disable.mutateAsync())}
            >
              {m.settings_sync_disable()}
            </Button>
          ) : null}
          <Button
            disabled={running || action.isPending || !settings}
            onClick={() => save(action.mutateAsync())}
          >
            {settings?.enabled
              ? m.settings_sync_now()
              : m.settings_sync_enable()}
          </Button>
        </SettingsCardFooter>
      ) : null}
    </SettingsCard>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-on-surface-variant">{label}</span>
      <span className="truncate">{value}</span>
    </div>
  )
}

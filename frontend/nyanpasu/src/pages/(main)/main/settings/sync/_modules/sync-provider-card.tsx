import CloudUploadRounded from '~icons/material-symbols/cloud-upload-rounded'
import { useEffect, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Input } from '@nyanpasu/ui/input'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { SwitchItem } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSync } from '@nyanpasu/query'
import type { ProviderConfig, ProviderKind } from '@nyanpasu/rpc/types'
import { PlatformDesktop } from '@nyanpasu/platform/react'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../../_modules/settings-card'
import SyncCardHeader from './sync-card-header'

const DEFAULT_INTERVAL_MINUTES = 30

export default function SyncProviderCard() {
  return (
    <PlatformDesktop
      fallback={
        <SettingsCard data-slot="sync-provider-card">
          <SyncCardHeader icon={CloudUploadRounded}>
            {m.settings_sync_provider_title()}
          </SyncCardHeader>
          <SettingsCardContent className="pt-2">
            <p className="text-on-surface-variant text-sm">
              {m.settings_sync_desktop_only()}
            </p>
          </SettingsCardContent>
        </SettingsCard>
      }
    >
      <SyncProviderControls />
    </PlatformDesktop>
  )
}

function SyncProviderControls() {
  const sync = useSync()
  const view = sync.settings.data
  const settings = view?.settings

  const [kind, setKind] = useState<ProviderKind>('webdav')
  const [endpoint, setEndpoint] = useState('')
  const [root, setRoot] = useState('')
  const [username, setUsername] = useState('')
  const [bucket, setBucket] = useState('')
  const [region, setRegion] = useState('auto')
  const [accessKeyId, setAccessKeyId] = useState('')
  const [password, setPassword] = useState('')
  const [secretKey, setSecretKey] = useState('')
  const [deviceName, setDeviceName] = useState('')
  const [intervalMinutes, setIntervalMinutes] = useState(
    DEFAULT_INTERVAL_MINUTES,
  )
  const [syncOnChange, setSyncOnChange] = useState(true)

  useEffect(() => {
    if (!settings) return
    setDeviceName(settings.device_name)
    setIntervalMinutes(settings.interval_minutes)
    setSyncOnChange(settings.sync_on_change)
    const provider = settings.provider
    if (!provider) return
    setKind(provider.kind)
    if (provider.kind === 'webdav') {
      setEndpoint(provider.endpoint)
      setRoot(provider.root)
      setUsername(provider.username ?? '')
    } else if (provider.kind === 's3') {
      setEndpoint(provider.endpoint ?? '')
      setRoot(provider.root)
      setBucket(provider.bucket)
      setRegion(provider.region)
      setAccessKeyId(provider.access_key_id)
    } else {
      setRoot(provider.root)
    }
  }, [settings])

  if (!view) return null

  const kinds = view.provider_kinds
  const availableKind = kinds.includes(kind) ? kind : kinds[0]
  if (!availableKind) {
    return (
      <SettingsCard data-slot="sync-provider-card">
        <SyncCardHeader icon={CloudUploadRounded}>
          {m.settings_sync_provider_title()}
        </SyncCardHeader>
        <SettingsCardContent className="pt-2">
          <p className="text-on-surface-variant text-sm">
            {m.settings_sync_no_providers()}
          </p>
        </SettingsCardContent>
      </SettingsCard>
    )
  }

  const provider: ProviderConfig =
    availableKind === 'webdav'
      ? {
          kind: 'webdav',
          endpoint,
          root,
          username: username === '' ? null : username,
        }
      : availableKind === 's3'
        ? {
            kind: 's3',
            endpoint: endpoint === '' ? null : endpoint,
            region,
            bucket,
            root,
            access_key_id: accessKeyId,
            virtual_host_style: false,
          }
        : { kind: 'fs', root }

  const secrets =
    password === '' && secretKey === ''
      ? null
      : {
          password: password === '' ? null : password,
          secret_access_key: secretKey === '' ? null : secretKey,
        }

  const nextSettings = settings
    ? {
        ...settings,
        provider,
        device_name: deviceName,
        interval_minutes: intervalMinutes,
        sync_on_change: syncOnChange,
      }
    : null

  const save = async (operation: Promise<unknown>) => {
    try {
      await operation
      message(m.settings_sync_saved())
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <SettingsCard data-slot="sync-provider-card">
      <SyncCardHeader icon={CloudUploadRounded}>
        {m.settings_sync_provider_title()}
      </SyncCardHeader>
      <SettingsCardContent className="gap-3 pt-2">
        <SegmentedButton
          value={availableKind}
          onValueChange={(value) => setKind(value as ProviderKind)}
        >
          {kinds.map((available) => (
            <SegmentedButtonItem key={available} value={available}>
              {available}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>

        {availableKind !== 'fs' ? (
          <Input
            label={m.settings_sync_endpoint()}
            value={endpoint}
            onChange={(event) => setEndpoint(event.target.value)}
          />
        ) : null}

        {availableKind === 's3' ? (
          <>
            <Input
              label={m.settings_sync_bucket()}
              value={bucket}
              onChange={(event) => setBucket(event.target.value)}
            />
            <Input
              label={m.settings_sync_region()}
              value={region}
              onChange={(event) => setRegion(event.target.value)}
            />
            <Input
              label={m.settings_sync_access_key()}
              value={accessKeyId}
              onChange={(event) => setAccessKeyId(event.target.value)}
            />
          </>
        ) : null}

        <Input
          label={m.settings_sync_root()}
          value={root}
          onChange={(event) => setRoot(event.target.value)}
        />

        {availableKind === 'webdav' ? (
          <>
            <Input
              label={m.settings_sync_username()}
              value={username}
              onChange={(event) => setUsername(event.target.value)}
            />
            <Input
              type="password"
              autoComplete="off"
              label={m.settings_sync_password()}
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </>
        ) : null}

        {availableKind === 's3' ? (
          <Input
            type="password"
            autoComplete="off"
            label={m.settings_sync_secret_key()}
            value={secretKey}
            onChange={(event) => setSecretKey(event.target.value)}
          />
        ) : null}

        <p className="text-on-surface-variant text-xs">
          {view.credentials_present
            ? m.settings_sync_credentials_present()
            : m.settings_sync_credentials_absent()}
        </p>

        <Input
          label={m.settings_sync_device_name()}
          value={deviceName}
          onChange={(event) => setDeviceName(event.target.value)}
        />

        <Input
          type="number"
          min={0}
          label={m.settings_sync_interval()}
          value={String(intervalMinutes)}
          onChange={(event) =>
            setIntervalMinutes(Math.max(0, Number(event.target.value) || 0))
          }
        />

        <SwitchItem
          className="rounded-[20px]"
          aria-label={m.settings_sync_on_change()}
          checked={syncOnChange}
          onCheckedChange={setSyncOnChange}
          loading={sync.configure.isPending}
        />

        <p className="text-on-surface-variant text-xs">
          {m.settings_sync_unencrypted_notice()}
        </p>
      </SettingsCardContent>
      <SettingsCardFooter className="justify-end gap-2">
        <Button
          variant="stroked"
          disabled={sync.testConnection.isPending || !nextSettings}
          onClick={() =>
            nextSettings &&
            save(
              sync.testConnection.mutateAsync({
                settings: nextSettings,
                secrets,
              }),
            )
          }
        >
          {m.settings_sync_test_connection()}
        </Button>
        <Button
          disabled={sync.configure.isPending || !nextSettings}
          onClick={() =>
            nextSettings &&
            save(sync.configure.mutateAsync({ settings: nextSettings, secrets }))
          }
        >
          {m.settings_sync_save()}
        </Button>
      </SettingsCardFooter>
    </SettingsCard>
  )
}

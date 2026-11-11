import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import PauseCircleRounded from '~icons/material-symbols/pause-circle-rounded'
import SyncRounded from '~icons/material-symbols/sync-rounded'
import { useEffect, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  unwrapQueryOptions,
  useClashSetting,
  useQueryApi,
  useSetting,
} from '@nyanpasu/query'
import type {
  NetworkTransparentProxyMode,
  TransparentProxyConfig,
  TransparentProxyMode,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { OptionalPortConfig } from './proxy-port-config'

type EffectiveTransparentProxyConfig = Required<TransparentProxyConfig>

const DEFAULT_CONFIG: EffectiveTransparentProxyConfig = {
  mode: 'disabled',
  local: true,
  interfaces: [],
  ipv6: false,
}

const STATUS_QUERY_KEY = ['getTransparentProxyStatus'] as const

const reportError = (error: unknown) =>
  message(formatError(error), { title: 'Error', kind: 'error', error })

function GatewayInterfacesConfig({
  interfaces,
  onApply,
}: {
  interfaces: string[]
  onApply: (interfaces: string[]) => Promise<void>
}) {
  const [open, setOpen] = useState(false)
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)

  const handleOpenChange = (next: boolean) => {
    if (next) setValue(interfaces.join(', '))
    setOpen(next)
  }

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const next = [
      ...new Set(
        value
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean),
      ),
    ]
    setSaving(true)
    try {
      await onApply(next)
      setOpen(false)
    } catch (error) {
      reportError(error)
    } finally {
      setSaving(false)
    }
  }

  return (
    <SettingsCard data-slot="transparent-proxy-interfaces-setting">
      <Modal open={open} onOpenChange={handleOpenChange}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_transparent_proxy_interfaces_label()}
                  </ItemLabelText>
                  <ItemLabelDescription>
                    {interfaces.length > 0
                      ? interfaces.join(', ')
                      : m.settings_clash_transparent_proxy_interfaces_description()}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <ModalContent>
          <Card className="flex w-[min(24rem,calc(100vw-2rem))] flex-col">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_transparent_proxy_interfaces_label()}
              </ModalTitle>
            </CardHeader>
            <form onSubmit={handleSubmit}>
              <CardContent className="flex flex-col gap-2">
                <Input
                  variant="outlined"
                  label={m.settings_clash_transparent_proxy_interfaces_label()}
                  aria-label={m.settings_clash_transparent_proxy_interfaces_label()}
                  aria-describedby="transparent-proxy-interfaces-description"
                  value={value}
                  onChange={(event) => setValue(event.target.value)}
                  disabled={saving}
                />
                <ItemLabelDescription
                  id="transparent-proxy-interfaces-description"
                  className="px-4"
                >
                  {m.settings_clash_transparent_proxy_interfaces_description()}{' '}
                  {m.settings_clash_transparent_proxy_interfaces_placeholder()}
                </ItemLabelDescription>
              </CardContent>
              <CardFooter className="gap-2">
                <Button type="submit" variant="flat" loading={saving}>
                  {m.common_apply()}
                </Button>
                <ModalClose disabled={saving}>{m.common_close()}</ModalClose>
              </CardFooter>
            </form>
          </Card>
        </ModalContent>
      </Modal>
    </SettingsCard>
  )
}

export default function TransparentProxyConfigSettings() {
  const core = useSetting('core')
  const serviceMode = useSetting('enable_service_mode')
  const transparentProxy = useClashSetting('transparent_proxy')
  const redirPort = useClashSetting('redir_port')
  const tproxyPort = useClashSetting('tproxy_port')
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const statusOptions = api.queries.getTransparentProxyStatus()
  const supportedCore = core.value === 'mihomo' || core.value === 'mihomo-alpha'
  const statusQuery = useQuery({
    ...unwrapQueryOptions(statusOptions, statusOptions.queryFn!),
    enabled: supportedCore,
    refetchInterval: serviceMode.value ? 2000 : false,
  })

  useEffect(() => {
    if (!supportedCore) return
    queryClient
      .invalidateQueries({ queryKey: STATUS_QUERY_KEY })
      .catch(() => undefined)
  }, [core.value, serviceMode.value, queryClient, supportedCore])

  const config: EffectiveTransparentProxyConfig = {
    ...DEFAULT_CONFIG,
    ...transparentProxy.value,
    interfaces: transparentProxy.value?.interfaces ?? DEFAULT_CONFIG.interfaces,
  }
  if (!supportedCore) {
    return null
  }

  const activationDisabled =
    config.mode === 'disabled' &&
    (!serviceMode.value || statusQuery.data?.supported !== true)
  const refreshStatus = () =>
    queryClient.invalidateQueries({ queryKey: STATUS_QUERY_KEY })

  const handleModeChange = async (mode: string) => {
    if (mode !== 'disabled' && activationDisabled) return
    if (mode !== 'disabled' && mode !== 'redir' && mode !== 'tproxy') return

    try {
      if (mode === 'redir' && !redirPort.value) {
        await redirPort.upsert({ kind: 'fixed', start_port: 7893 })
      }
      if (mode === 'tproxy' && !tproxyPort.value) {
        await tproxyPort.upsert({ kind: 'fixed', start_port: 7894 })
      }

      await transparentProxy.upsert({ ...config, mode })
      await refreshStatus()
    } catch (error) {
      reportError(error)
    }
  }

  const handleConfigChange = async (
    patch: Partial<EffectiveTransparentProxyConfig>,
  ) => {
    try {
      await transparentProxy.upsert({ ...config, ...patch })
      await refreshStatus()
    } catch (error) {
      reportError(error)
    }
  }

  const handleConfigToggle = (
    patch: Partial<EffectiveTransparentProxyConfig>,
  ) => {
    handleConfigChange(patch).catch(() => undefined)
  }

  const modeLabels = {
    disabled: m.settings_clash_transparent_proxy_mode_disabled(),
    redir: m.settings_clash_transparent_proxy_mode_redir(),
    tproxy: m.settings_clash_transparent_proxy_mode_tproxy(),
  } satisfies Record<TransparentProxyMode, string>

  const actualMode: NetworkTransparentProxyMode = statusQuery.data?.active
    ? (statusQuery.data.mode ?? 'disabled')
    : 'disabled'
  const statusError = statusQuery.error
    ? formatError(statusQuery.error)
    : (statusQuery.data?.error ?? null)
  const actualStatusLabel = statusQuery.isPending
    ? m.settings_clash_transparent_proxy_status_loading()
    : statusQuery.isError
      ? m.settings_clash_transparent_proxy_status_unavailable()
      : !statusQuery.data?.supported
        ? m.settings_clash_transparent_proxy_status_unsupported()
        : modeLabels[actualMode]
  const modeApplied =
    !statusQuery.isPending &&
    !statusQuery.isError &&
    statusQuery.data?.supported === true &&
    statusError === null &&
    actualMode === config.mode
  const statusUnavailable =
    statusQuery.isError ||
    statusError !== null ||
    (!statusQuery.isPending && statusQuery.data?.supported !== true)
  const StatusIcon = statusUnavailable
    ? ErrorRounded
    : !modeApplied
      ? SyncRounded
      : actualMode === 'disabled'
        ? PauseCircleRounded
        : CheckCircleRounded

  return (
    <div data-slot="transparent-proxy-settings-container">
      <SettingsLabel>
        {m.settings_clash_transparent_proxy_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard data-slot="transparent-proxy-config-card">
          <SettingsCardContent className="flex flex-col gap-4 py-4">
            <div className="flex flex-col gap-2">
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_clash_transparent_proxy_mode_label()}
                </ItemLabelText>
                <ItemLabelDescription>
                  {m.settings_clash_transparent_proxy_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <SegmentedButton
                value={config.mode}
                onValueChange={handleModeChange}
                disabled={transparentProxy.isPending}
              >
                {(['disabled', 'redir', 'tproxy'] as const).map((mode) => (
                  <SegmentedButtonItem
                    key={mode}
                    value={mode}
                    disabled={
                      mode !== 'disabled' &&
                      activationDisabled &&
                      config.mode === 'disabled'
                    }
                  >
                    {modeLabels[mode]}
                  </SegmentedButtonItem>
                ))}
              </SegmentedButton>

              {!serviceMode.value && (
                <p className="text-on-surface-variant text-sm">
                  {m.settings_clash_transparent_proxy_service_mode_required()}
                </p>
              )}
            </div>
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard data-slot="transparent-proxy-runtime-status">
          <SettingsCardContent aria-live="polite" className="gap-4">
            <div className="flex flex-wrap items-center gap-4">
              <div
                className={cn(
                  'grid size-12 shrink-0 place-items-center rounded-full',
                  statusUnavailable
                    ? 'bg-error-container text-on-error-container'
                    : modeApplied && actualMode !== 'disabled'
                      ? 'bg-primary-container text-on-primary-container'
                      : 'bg-secondary-container text-on-secondary-container',
                )}
              >
                <StatusIcon className="size-6" aria-hidden="true" />
              </div>

              <div className="min-w-0 flex-1">
                <p className="text-on-surface-variant text-sm">
                  {m.settings_clash_transparent_proxy_actual_status()}
                </p>
                <p className="text-lg font-medium wrap-break-word">
                  {actualStatusLabel}
                </p>
                {!statusQuery.isPending && !statusUnavailable && (
                  <p className="text-on-surface-variant text-sm">
                    {modeApplied
                      ? m.settings_clash_transparent_proxy_status_applied()
                      : m.settings_clash_transparent_proxy_status_pending()}
                  </p>
                )}
              </div>

              <div className="bg-surface-variant/40 text-on-surface-variant flex items-center gap-2 rounded-full px-4 py-2 text-sm">
                <span>{m.settings_clash_transparent_proxy_desired_mode()}</span>
                <span className="text-on-surface font-medium">
                  {modeLabels[config.mode]}
                </span>
              </div>
            </div>
            {statusError && (
              <p className="text-error text-sm wrap-break-word" role="alert">
                {statusError}
              </p>
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard data-slot="transparent-proxy-local-setting">
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_clash_transparent_proxy_local_label()}
                </ItemLabelText>
                <ItemLabelDescription>
                  {m.settings_clash_transparent_proxy_local_description()}
                </ItemLabelDescription>
              </ItemLabel>
              <Switch
                checked={config.local}
                onCheckedChange={(local) => handleConfigToggle({ local })}
                loading={transparentProxy.isPending}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>

        <GatewayInterfacesConfig
          interfaces={config.interfaces}
          onApply={async (interfaces) => {
            await transparentProxy.upsert({ ...config, interfaces })
            await refreshStatus()
          }}
        />

        <SettingsCard data-slot="transparent-proxy-ipv6-setting">
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_clash_transparent_proxy_ipv6_label()}
                </ItemLabelText>
                <ItemLabelDescription>
                  {m.settings_clash_transparent_proxy_ipv6_description()}
                </ItemLabelDescription>
              </ItemLabel>
              <Switch
                checked={config.ipv6}
                onCheckedChange={(ipv6) => handleConfigToggle({ ipv6 })}
                loading={transparentProxy.isPending}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>

        <OptionalPortConfig field="redir_port" onApplied={refreshStatus} />

        <OptionalPortConfig field="tproxy_port" onApplied={refreshStatus} />
      </SettingsGroup>
    </div>
  )
}

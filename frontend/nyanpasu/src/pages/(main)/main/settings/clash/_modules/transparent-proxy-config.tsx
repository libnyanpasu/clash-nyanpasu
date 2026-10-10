import { useEffect, useState } from 'react'
import { Input } from '@nyanpasu/ui/input'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { isLinux } from '@nyanpasu/platform'
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
} from '../../_modules/settings-card'
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
    enabled: isLinux && supportedCore,
    refetchInterval: serviceMode.value ? 2000 : false,
  })

  useEffect(() => {
    if (!isLinux || !supportedCore) return
    queryClient
      .invalidateQueries({ queryKey: STATUS_QUERY_KEY })
      .catch(() => undefined)
  }, [core.value, serviceMode.value, queryClient, supportedCore])

  const config: EffectiveTransparentProxyConfig = {
    ...DEFAULT_CONFIG,
    ...transparentProxy.value,
    interfaces: transparentProxy.value?.interfaces ?? DEFAULT_CONFIG.interfaces,
  }
  const [interfacesText, setInterfacesText] = useState(
    config.interfaces.join(', '),
  )

  useEffect(() => {
    setInterfacesText(config.interfaces.join(', '))
  }, [config.interfaces])

  if (!isLinux || !supportedCore) {
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

  const handleInterfacesBlur = () => {
    const interfaces = [
      ...new Set(
        interfacesText
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean),
      ),
    ]
    setInterfacesText(interfaces.join(', '))
    handleConfigChange({ interfaces })
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

            <div
              className="flex flex-col gap-2"
              data-slot="transparent-proxy-runtime-status"
              aria-live="polite"
            >
              <ItemContainer>
                <ItemLabelText>
                  {m.settings_clash_transparent_proxy_desired_mode()}
                </ItemLabelText>
                <span>{modeLabels[config.mode]}</span>
              </ItemContainer>
              <ItemContainer>
                <ItemLabelText>
                  {m.settings_clash_transparent_proxy_actual_status()}
                </ItemLabelText>
                <span>{actualStatusLabel}</span>
              </ItemContainer>
              {!statusQuery.isPending && !statusQuery.isError && (
                <p className="text-on-surface-variant text-sm">
                  {modeApplied
                    ? m.settings_clash_transparent_proxy_status_applied()
                    : m.settings_clash_transparent_proxy_status_pending()}
                </p>
              )}
              {statusError && (
                <p className="text-error text-sm" role="alert">
                  {statusError}
                </p>
              )}
            </div>

            <ItemContainer data-slot="transparent-proxy-local-setting">
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

            <Input
              variant="outlined"
              label={m.settings_clash_transparent_proxy_interfaces_label()}
              value={interfacesText}
              placeholder={m.settings_clash_transparent_proxy_interfaces_placeholder()}
              onChange={(event) => setInterfacesText(event.target.value)}
              onBlur={handleInterfacesBlur}
              disabled={transparentProxy.isPending}
              aria-describedby="transparent-proxy-interfaces-description"
            >
              <span
                id="transparent-proxy-interfaces-description"
                className="text-on-surface-variant px-4 text-sm"
              >
                {m.settings_clash_transparent_proxy_interfaces_description()}
              </span>
            </Input>

            <ItemContainer data-slot="transparent-proxy-ipv6-setting">
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

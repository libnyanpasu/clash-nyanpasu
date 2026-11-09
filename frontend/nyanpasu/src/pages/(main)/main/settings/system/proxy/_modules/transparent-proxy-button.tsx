import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { ProxyButton } from '@/components/settings/system-proxy'
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
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'

export default function TransparentProxyButton() {
  const core = useSetting('core')
  const serviceMode = useSetting('enable_service_mode')
  const tun = useClashSetting('enable_tun_mode')
  const transparentProxy = useClashSetting('transparent_proxy')
  const tproxyPort = useClashSetting('tproxy_port')
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const supportedCore = core.value === 'mihomo' || core.value === 'mihomo-alpha'
  const active =
    transparentProxy.value?.mode !== undefined &&
    transparentProxy.value.mode !== 'disabled'
  const statusOptions = api.queries.getTransparentProxyStatus()
  const status = useQuery({
    ...unwrapQueryOptions(statusOptions, statusOptions.queryFn!),
    enabled: isLinux && supportedCore && Boolean(serviceMode.value),
    refetchInterval: serviceMode.value ? 2000 : false,
  })

  const task = useBlockTask('transparent-proxy', async () => {
    if (!active && !tproxyPort.value) {
      await tproxyPort.upsert({ kind: 'fixed', start_port: 7894 })
    }
    await transparentProxy.upsert({
      local: true,
      interfaces: [],
      ipv6: false,
      ...transparentProxy.value,
      mode: active ? 'disabled' : 'tproxy',
    })
    await queryClient.invalidateQueries({ queryKey: statusOptions.queryKey })
  })

  if (!isLinux || !supportedCore) return null

  const disabled =
    !active &&
    (!serviceMode.value || status.data?.supported !== true || tun.value)
  const reason = !serviceMode.value
    ? m.settings_clash_transparent_proxy_service_mode_required()
    : tun.value && !active
      ? m.error_transparent_proxy_tun_conflict()
      : status.isError
        ? m.settings_clash_transparent_proxy_status_unavailable()
        : status.isPending && serviceMode.value
          ? m.settings_clash_transparent_proxy_status_loading()
          : status.data && !status.data.supported
            ? m.settings_clash_transparent_proxy_status_unsupported()
            : undefined

  const applied = active
    ? status.data?.active && status.data.mode === transparentProxy.value?.mode
    : !status.data?.active

  const handleClick = () => {
    task.execute().catch((error: unknown) => {
      message(formatError(error), { title: 'Error', kind: 'error', error })
    })
  }

  return (
    <div className="col-span-2 flex flex-col gap-2">
      <ProxyButton
        isActive={active}
        loading={task.isPending}
        disabled={disabled}
        onClick={handleClick}
      >
        <ShieldRounded />
        <span>{m.settings_clash_transparent_proxy_label()}</span>
        <span className="text-sm font-normal">
          {transparentProxy.value?.mode === 'redir'
            ? m.settings_clash_transparent_proxy_mode_redir()
            : m.settings_clash_transparent_proxy_mode_tproxy()}
        </span>
      </ProxyButton>

      {reason && <p className="text-on-surface-variant text-sm">{reason}</p>}

      {status.data?.supported && (active || status.data.active) && (
        <p className="text-on-surface-variant text-sm">
          {status.data.error ??
            (applied
              ? m.settings_clash_transparent_proxy_status_applied()
              : m.settings_clash_transparent_proxy_status_pending())}
        </p>
      )}

      <Link
        className="text-primary text-sm underline"
        to="/main/settings/clash/network"
      >
        {m.settings_label_network()}
      </Link>
    </div>
  )
}

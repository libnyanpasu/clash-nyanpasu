import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { isLinux, isTauri } from '@nyanpasu/platform'
import {
  unwrapQueryOptions,
  useClashSetting,
  useClashSettings,
  useQueryApi,
  useSetting,
} from '@nyanpasu/query'
import { useQuery, useQueryClient } from '@tanstack/react-query'

export const useSystemProxy = () => {
  const systemProxy = useSetting('enable_system_proxy')

  const blockTask = useBlockTask('system-proxy', async () => {
    await systemProxy.upsert(!systemProxy.value)
  })

  return {
    ...blockTask,
    isActive: Boolean(systemProxy.value),
  }
}

export const useTunMode = () => {
  const tunMode = useClashSetting('enable_tun_mode')
  const transparentProxy = useClashSetting('transparent_proxy')
  const disabled =
    !tunMode.value &&
    transparentProxy.value?.mode != null &&
    transparentProxy.value.mode !== 'disabled'

  const blockTask = useBlockTask('tun-mode', async () => {
    await tunMode.upsert(!tunMode.value)
  })

  return {
    ...blockTask,
    isActive: Boolean(tunMode.value),
    disabled,
  }
}

export const useTransparentProxy = () => {
  const core = useSetting('core')
  const serviceMode = useSetting('enable_service_mode')
  const clash = useClashSettings()
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const supportedCore = core.value === 'mihomo' || core.value === 'mihomo-alpha'
  const visible = isLinux && isTauri() && supportedCore
  const config = clash.query.data
  const mode = config?.transparent_proxy?.mode ?? 'disabled'
  const isActive = mode !== 'disabled'
  const statusOptions = api.queries.getTransparentProxyStatus()
  const status = useQuery({
    ...unwrapQueryOptions(statusOptions, statusOptions.queryFn!),
    enabled: visible && Boolean(serviceMode.value),
    refetchInterval: serviceMode.value ? 2000 : false,
  })
  const task = useBlockTask('transparent-proxy', async () => {
    if (!config) return
    await clash.upsert.mutateAsync({
      transparent_proxy: { mode: isActive ? 'disabled' : 'tproxy' },
      ...(!isActive && !config.tproxy_port
        ? { tproxy_port: { kind: 'fixed', start_port: 7894 } }
        : {}),
    })
    await queryClient.invalidateQueries({ queryKey: statusOptions.queryKey })
  })
  const disabled =
    !config ||
    (!isActive &&
      (!serviceMode.value ||
        status.data?.supported !== true ||
        config.enable_tun_mode))
  const reason = !serviceMode.value
    ? m.settings_clash_transparent_proxy_service_mode_required()
    : config?.enable_tun_mode && !isActive
      ? m.error_transparent_proxy_tun_conflict()
      : status.isError
        ? m.settings_clash_transparent_proxy_status_unavailable()
        : status.isPending && serviceMode.value
          ? m.settings_clash_transparent_proxy_status_loading()
          : status.data && !status.data.supported
            ? m.settings_clash_transparent_proxy_status_unsupported()
            : undefined
  const applied = isActive
    ? status.data?.active && status.data.mode === mode
    : !status.data?.active

  const execute = async () => {
    try {
      await task.execute()
    } catch (error) {
      message(formatError(error), { title: 'Error', kind: 'error', error })
    }
  }
  return {
    visible,
    serviceModeEnabled: Boolean(serviceMode.value),
    isActive,
    isPending: task.isPending,
    disabled,
    reason,
    applied,
    status: status.data,
    mode,
    execute,
  }
}

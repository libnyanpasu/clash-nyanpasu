import { useEffect, useRef } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { useClashSetting, useSetting } from './use-settings'

/**
 * Probes the public address the app's own requests leave from, over whatever
 * route its settings give them: the system proxy, TUN, local transparent
 * capture or none. It probes once detection is on and probes again when that
 * route changes, since the earlier answer no longer holds.
 */
export function useRouteEgressProbe() {
  const api = useQueryApi()

  const { value: allowed } = useSetting('enable_local_ip_probe')
  const { value: systemProxy } = useSetting('enable_system_proxy')
  const { value: tunEnabled } = useClashSetting('enable_tun_mode')
  const { value: transparentProxy } = useClashSetting('transparent_proxy')
  const localCapture = Boolean(
    transparentProxy?.local &&
    transparentProxy.mode != null &&
    transparentProxy.mode !== 'disabled',
  )
  const loaded = systemProxy !== undefined && tunEnabled !== undefined
  const route = `${systemProxy}:${tunEnabled}:${localCapture}`

  const mutation = useMutation({
    mutationKey: api.mutations.probeRouteEgress().mutationKey,
    mutationFn: async () => await api.probeRouteEgress(),
    retry: false,
  })
  const { mutate } = mutation
  const probedRoute = useRef<string | null>(null)

  useEffect(() => {
    if (allowed !== true || !loaded) {
      probedRoute.current = null
      return
    }
    // StrictMode may rerun the effect with already-cached configuration.
    if (probedRoute.current !== route) {
      probedRoute.current = route
      mutate()
    }
  }, [allowed, loaded, route, mutate])

  return mutation
}

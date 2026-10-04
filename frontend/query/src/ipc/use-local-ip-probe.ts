import { useEffect, useRef } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import { useMutation } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { useClashSetting, useSetting } from './use-settings'

/** Probe once when a traffic view becomes eligible; reporting polls never trigger probes. */
export function useLocalIpProbe(enabled = true) {
  const api = useQueryApi()

  const { value: allowed } = useSetting('enable_local_ip_probe')
  const { value: tunEnabled } = useClashSetting('enable_tun_mode')

  const mutation = useMutation({
    mutationKey: api.mutations.probeDirectEgress.mutationKey,
    mutationFn: async () => unwrapResult(await api.probeDirectEgress()),
    retry: false,
  })
  const { mutate } = mutation
  const probed = useRef(false)

  useEffect(() => {
    if (!enabled || allowed !== true || tunEnabled !== false) {
      probed.current = false
      return
    }
    // StrictMode may rerun the effect with already-cached configuration.
    if (!probed.current) {
      probed.current = true
      mutate()
    }
  }, [enabled, allowed, tunEnabled, mutate])

  return mutation
}

import { unwrapResult } from '@nyanpasu/rpc'
import type {
  NetworkAutomationStatus,
  RpcNetworkAutomationStatus,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'
import { useFullSystemSetting } from './use-settings'

/** The status is read by polling: the host has no network-change event. */
const STATUS_REFETCH_INTERVAL = 5_000

/**
 * The network reading a command reply holds. A caller without `sensitive_read`
 * is answered with a summary that leaves out the Wi-Fi name, the gateway and
 * the matched rule, so there is no reading to use.
 */
export const networkReadingOf = (
  reply: RpcNetworkAutomationStatus,
): NetworkAutomationStatus | null => ('network' in reply ? reply : null)

/**
 * The saved network rules. A patch replaces the policy as a whole, so `save`
 * takes the complete next policy. A rule switches the system proxy, TUN and
 * transparent proxy, so the policy is written under the `system` scope. The
 * rules name Wi-Fi networks and gateway MAC addresses, so the editor reads the
 * full config (`sensitive_read`), which the safe settings view reduces to a
 * flag.
 */
export const useNetworkAutomation = () => {
  const setting = useFullSystemSetting('network_automation')

  return {
    policy: setting.value,
    save: setting.upsert,
    isPending: setting.isPending,
  }
}

/**
 * The network the host is on and what the rules last did about it. Desktop
 * only: the status holds the live Wi-Fi name and gateway address.
 */
export const useNetworkAutomationStatus = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const statusQuery = api.queries.getNetworkAutomationStatus()
  const applyMutation = api.mutations.applyNetworkAutomation
  const requestMutation = api.mutations.requestWifiNameAccess

  const query = useQuery({
    ...unwrapQueryOptions(statusQuery, statusQuery.queryFn!),
    refetchInterval: STATUS_REFETCH_INTERVAL,
  })

  // Applying writes the capture switches, so their settings are read again.
  const applyNow = useMutation({
    mutationKey: applyMutation.mutationKey,
    mutationFn: async () =>
      unwrapResult(await invokeMutation(applyMutation, [])),
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: statusQuery.queryKey }),
        queryClient.invalidateQueries({
          queryKey: api.queries.getAppConfig().queryKey,
        }),
        queryClient.invalidateQueries({
          queryKey: api.queries.getClashConfig().queryKey,
        }),
      ]),
  })

  // Reads the Wi-Fi name once, which is what asks the system for the location
  // access it gates the name behind. The status it returns carries that
  // reading, so a caller can use the name without waiting for the next poll.
  const requestWifiName = useMutation({
    mutationKey: requestMutation.mutationKey,
    mutationFn: async () =>
      networkReadingOf(unwrapResult(await invokeMutation(requestMutation, []))),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: statusQuery.queryKey }),
  })

  return { query, applyNow, requestWifiName }
}

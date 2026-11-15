import { unwrapResult } from '@nyanpasu/rpc'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'
import { useSetting } from './use-settings'

/** The status is read by polling: the host has no network-change event. */
const STATUS_REFETCH_INTERVAL = 5_000

/**
 * The saved network rules. A patch replaces the policy as a whole, so `save`
 * takes the complete next policy.
 */
export const useNetworkAutomation = () => {
  const setting = useSetting('network_automation')

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

  return { query, applyNow }
}

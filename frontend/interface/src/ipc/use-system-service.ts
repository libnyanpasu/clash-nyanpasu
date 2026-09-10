import { unwrapResult } from '@interface/utils'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { mutations, queries } from './bindings'
import { invokeMutation, unwrapQueryOptions } from './query-options'

export type ServiceType = 'install' | 'uninstall' | 'start' | 'stop'

/**
 * Custom hook to fetch and manage the system service status using TanStack Query.
 *
 * @returns An object containing the query result for the system service status.
 */
export const useSystemService = () => {
  const queryClient = useQueryClient()
  const statusQuery = queries.statusService()
  const installService = mutations.installService
  const uninstallService = mutations.uninstallService
  const startService = mutations.startService
  const stopService = mutations.stopService

  const query = useQuery(unwrapQueryOptions(statusQuery, statusQuery.queryFn!))

  const upsert = useMutation({
    mutationKey: installService.mutationKey,
    mutationFn: async (type: ServiceType) => {
      switch (type) {
        case 'install':
          unwrapResult(await invokeMutation(installService, []))
          break

        case 'uninstall':
          unwrapResult(await invokeMutation(uninstallService, []))
          break

        case 'start':
          unwrapResult(await invokeMutation(startService, []))
          break

        case 'stop':
          unwrapResult(await invokeMutation(stopService, []))
          break
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: statusQuery.queryKey,
      })
    },
  })

  return {
    query,
    upsert,
  }
}

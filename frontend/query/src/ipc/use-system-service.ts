import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation } from './query-options'

export type ServiceType = 'install' | 'uninstall' | 'start' | 'stop'

/**
 * Custom hook to fetch and manage the system service status using TanStack Query.
 *
 * @returns An object containing the query result for the system service status.
 */
export const useSystemService = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const statusQuery = api.queries.statusService()
  const installService = api.mutations.installService()
  const uninstallService = api.mutations.uninstallService()
  const startService = api.mutations.startService()
  const stopService = api.mutations.stopService()

  const query = useQuery(statusQuery)

  const upsert = useMutation({
    mutationKey: installService.mutationKey,
    mutationFn: async (type: ServiceType) => {
      switch (type) {
        case 'install':
          await invokeMutation(installService)
          break

        case 'uninstall':
          await invokeMutation(uninstallService)
          break

        case 'start':
          await invokeMutation(startService)
          break

        case 'stop':
          await invokeMutation(stopService)
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

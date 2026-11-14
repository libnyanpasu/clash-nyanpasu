import { unwrapResult } from '@nyanpasu/rpc'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invokeMutation, invokeQuery } from '../ipc/query-options'
import { useSetting } from '../ipc/use-settings'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * The HTTP server's state and its remote-access switch.
 *
 * The status never carries a credential. The launch URL, which logs a caller in, is created on
 * demand by a mutation and handed straight to the opener, so it is never kept in the query cache.
 */
export function useHttpServer() {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const remoteAccess = useSetting('remote_access')

  const statusOptions = api.queries.getHttpServerStatus()
  const status = useQuery({
    queryKey: statusOptions.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(statusOptions)),
    refetchInterval: 2000,
  })

  const setCommand = api.mutations.setRemoteAccess
  const setEnabled = useMutation({
    mutationKey: setCommand.mutationKey,
    // Keep the whole outcome so committed_degraded remains a committed result.
    mutationFn: async (enabled: boolean) => {
      const current = remoteAccess.value
      if (current == null) {
        throw new Error('The remote access settings have not loaded yet.')
      }

      return unwrapResult(
        await invokeMutation(setCommand, [
          { ...current, http: { ...current.http, enabled } },
        ]),
      )
    },
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: statusOptions.queryKey }),
        queryClient.invalidateQueries({
          queryKey: api.queries.getAppConfig().queryKey,
        }),
      ]),
  })

  const launchCommand = api.mutations.createHttpLaunchUrl
  const openCommand = api.mutations.openWebUrl
  const openInBrowser = useMutation({
    mutationKey: launchCommand.mutationKey,
    mutationFn: async () => {
      const url = unwrapResult(await invokeMutation(launchCommand, []))
      if (url != null) {
        unwrapResult(await invokeMutation(openCommand, [url]))
      }
    },
  })

  return {
    status,
    /** The committed setting, not the listener: a failed bind leaves it on. */
    enabled:
      remoteAccess.value == null
        ? undefined
        : (remoteAccess.value.http?.enabled ?? false),
    setEnabled,
    openInBrowser,
  }
}

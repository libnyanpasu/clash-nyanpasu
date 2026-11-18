import { unwrapResult } from '@nyanpasu/rpc'
import type {
  CreateRemoteAccessTokenRequest,
  HttpAccessConfig,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invokeMutation, invokeQuery } from '../ipc/query-options'
import { useSetting } from '../ipc/use-settings'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * The HTTP server's state and its remote-access settings.
 *
 * The status never carries a credential; its certificate fingerprint is public to every TLS
 * client. The launch URL, which logs a caller in, is created on
 * demand by a mutation and handed straight to the opener, so it is never kept in the query cache.
 */
export function useRemoteAccess() {
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
  const update = useMutation({
    mutationKey: setCommand.mutationKey,
    // The command replaces the whole section, so a change is merged into the committed one. Keep
    // the whole outcome so committed_degraded remains a committed result.
    mutationFn: async (patch: HttpAccessConfig) => {
      const current = remoteAccess.value
      if (current == null) {
        throw new Error('The remote access settings have not loaded yet.')
      }

      return unwrapResult(
        await invokeMutation(setCommand, [
          { ...current, http: { ...current.http, ...patch } },
        ]),
      )
    },
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: statusOptions.queryKey }),
        queryClient.invalidateQueries({
          queryKey: api.queries.getAppSettings().queryKey,
        }),
        queryClient.invalidateQueries({
          queryKey: api.queries.getAppConfig().queryKey,
        }),
      ]),
  })

  const resetCertificateCommand = api.mutations.resetRemoteAccessTls
  const resetCertificate = useMutation({
    mutationKey: resetCertificateCommand.mutationKey,
    mutationFn: async () =>
      unwrapResult(await invokeMutation(resetCertificateCommand, [])),
    // The listener restarts with the new certificate, whose fingerprint the status reports.
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: statusOptions.queryKey }),
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
    /** The committed settings, not the listener: a failed bind leaves them on. */
    http:
      remoteAccess.value == null ? undefined : (remoteAccess.value.http ?? {}),
    update,
    resetCertificate,
    openInBrowser,
  }
}

/**
 * The IPv4 networks this computer is attached to, which the LAN listen scope may allow. Read
 * fresh on every mount, since interfaces such as a VPN come and go.
 */
export function useLocalNetworks({ enabled }: { enabled: boolean }) {
  const api = useQueryApi()

  const listOptions = api.queries.listLocalNetworks()

  return useQuery({
    queryKey: listOptions.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(listOptions)),
    enabled,
    staleTime: 0,
  })
}

/**
 * The access tokens' metadata and their administration. A token's secret is returned once, by the
 * creating mutation, and is not kept: the mutation is dropped as soon as nothing observes it.
 */
export function useRemoteAccessTokens() {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const listOptions = api.queries.listRemoteAccessTokens()
  const query = useQuery({
    queryKey: listOptions.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(listOptions)),
  })

  const createCommand = api.mutations.createRemoteAccessToken
  const create = useMutation({
    mutationKey: createCommand.mutationKey,
    mutationFn: async (request: CreateRemoteAccessTokenRequest) =>
      unwrapResult(await invokeMutation(createCommand, [request])),
    gcTime: 0,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: listOptions.queryKey }),
  })

  const revokeCommand = api.mutations.revokeRemoteAccessToken
  const revoke = useMutation({
    mutationKey: revokeCommand.mutationKey,
    mutationFn: async (id: string) =>
      unwrapResult(await invokeMutation(revokeCommand, [id])),
    // Revoking a token also ends the browser sessions it opened.
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: listOptions.queryKey }),
        queryClient.invalidateQueries({
          queryKey: api.queries.listRemoteBrowserSessions().queryKey,
        }),
      ]),
  })

  return { query, create, revoke }
}

/** The signed-in browsers' metadata and their revocation. */
export function useRemoteBrowserSessions() {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const listOptions = api.queries.listRemoteBrowserSessions()
  const query = useQuery({
    queryKey: listOptions.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(listOptions)),
    refetchInterval: 10_000,
  })

  const revokeCommand = api.mutations.revokeRemoteBrowserSession
  const revoke = useMutation({
    mutationKey: revokeCommand.mutationKey,
    mutationFn: async (id: string) =>
      unwrapResult(await invokeMutation(revokeCommand, [id])),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: listOptions.queryKey }),
  })

  return { query, revoke }
}

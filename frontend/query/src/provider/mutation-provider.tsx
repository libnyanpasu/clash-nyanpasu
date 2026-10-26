import { useCallback, useEffect, useMemo, type PropsWithChildren } from 'react'
import type { StateChanged } from '@nyanpasu/rpc'
import {
  useQueryClient,
  type InvalidateQueryFilters,
  type QueryKey,
} from '@tanstack/react-query'
import { useQueryApi } from './rpc-provider'

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const mutationKeys = useMemo(
    (): Record<StateChanged['kind'], QueryKey[]> => ({
      nyanpasu_config: [
        api.queries.getAppConfig().queryKey,
        api.queries.getSysProxy().queryKey,
        // The proxies snapshot is trimmed to the default latency test URL.
        api.queries.getProxies().queryKey,
      ],
      // Sent for the runtime too, after every apply, core start or restart.
      clash_config: [
        api.queries.clashApiGetVersion().queryKey,
        api.queries.getClashConfig().queryKey,
        api.queries.getClashInfo().queryKey,
        api.queries.clashApiGetConfigs().queryKey,
      ],
      profiles: [api.queries.getProfiles().queryKey],
      proxies: [
        api.queries.getProxies().queryKey,
        api.queries.clashApiGetProvidersProxies().queryKey,
      ],
    }),
    [api.queries],
  )

  // These change only through the events above, so they are kept until one
  // arrives instead of refetched by every page that mounts them. The clash
  // info is not: a runtime outcome nobody observed can drop its ports.
  const keptKeys = useMemo(
    () => [
      api.queries.getAppConfig().queryKey,
      api.queries.getClashConfig().queryKey,
      api.queries.clashApiGetVersion().queryKey,
      api.queries.getProfiles().queryKey,
      api.queries.getProxies().queryKey,
    ],
    [api.queries],
  )

  // Set while rendering, before the children's queries decide whether to
  // fetch on mount.
  useMemo(() => {
    for (const queryKey of keptKeys) {
      queryClient.setQueryDefaults(queryKey, { staleTime: Infinity })
    }
  }, [keptKeys, queryClient])

  const refetch = useCallback(
    async (filters?: InvalidateQueryFilters) => {
      // A read in flight may have started before the change, and its answer
      // would otherwise settle the query as fresh.
      await queryClient.cancelQueries(filters)
      await queryClient.invalidateQueries(filters)
    },
    [queryClient],
  )

  const refetchQueries = useCallback(
    (keys: readonly QueryKey[]) => {
      Promise.all(keys.map((queryKey) => refetch({ queryKey }))).catch(
        (error) => console.error(error),
      )
    },
    [refetch],
  )

  useEffect(() => {
    let disposed = false
    const stateChanges = api.events.stateChangedEvent.listen(({ payload }) => {
      refetchQueries(mutationKeys[payload.kind])
      if (payload.kind === 'profiles') {
        refetchQueries(
          payload.files.map((uid) => api.queries.readProfileFile(uid).queryKey),
        )
      }
    })
    // A change committed before the listener was registered went unheard, and
    // the children may already have read the kept queries.
    stateChanges
      .then(() => {
        if (!disposed) refetchQueries(keptKeys)
      })
      .catch(console.error)
    const unlisteners = [
      stateChanges,
      api.events.coreStatusChangedEvent.listen(() =>
        refetchQueries([api.queries.getCoreStatus().queryKey]),
      ),
      api.events.serviceStatusChangedEvent.listen(() =>
        refetchQueries([api.queries.statusService().queryKey]),
      ),
    ]
    return () => {
      disposed = true
      unlisteners.forEach((unlisten) =>
        unlisten.then((stop) => stop()).catch(console.error),
      )
    }
  }, [api, keptKeys, mutationKeys, refetchQueries])

  useEffect(
    () =>
      api.listenResync(() => {
        refetch().catch(console.error)
      }),
    [api, refetch],
  )

  return children
}

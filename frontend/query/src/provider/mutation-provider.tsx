import { useCallback, useEffect, useMemo, type PropsWithChildren } from 'react'
import type { StateChanged } from '@nyanpasu/rpc'
import { useQueryClient, type QueryKey } from '@tanstack/react-query'
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
      clash_config: [
        api.queries.clashApiGetVersion().queryKey,
        api.queries.getClashConfig().queryKey,
        api.queries.getClashInfo().queryKey,
        api.queries.clashApiGetConfigs().queryKey,
        api.queries.getProfiles().queryKey,
      ],
      profiles: [
        api.queries.clashApiGetVersion().queryKey,
        api.queries.getClashConfig().queryKey,
        api.queries.getClashInfo().queryKey,
        api.queries.getProfiles().queryKey,
      ],
      proxies: [
        api.queries.getProxies().queryKey,
        api.queries.clashApiGetProvidersProxies().queryKey,
      ],
    }),
    [api.queries],
  )

  const refetchQueries = useCallback(
    (keys: readonly QueryKey[]) => {
      Promise.all(
        keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      ).catch((error) => console.error(error))
    },
    [queryClient],
  )

  useEffect(() => {
    const unlisteners = [
      api.events.coreStatusChangedEvent.listen(() =>
        refetchQueries([api.queries.getCoreStatus().queryKey]),
      ),
      api.events.serviceStatusChangedEvent.listen(() =>
        refetchQueries([api.queries.statusService().queryKey]),
      ),
      api.events.stateChangedEvent.listen(({ payload }) =>
        refetchQueries(mutationKeys[payload.kind]),
      ),
    ]
    return () => {
      unlisteners.forEach((unlisten) =>
        unlisten.then((stop) => stop()).catch(console.error),
      )
    }
  }, [api, mutationKeys, refetchQueries])

  useEffect(
    () =>
      api.listenResync(() => {
        queryClient.invalidateQueries().catch(console.error)
      }),
    [api, queryClient],
  )

  return children
}

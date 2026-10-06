import { useCallback, useEffect, useMemo, type PropsWithChildren } from 'react'
import { useQueryClient, type QueryKey } from '@tanstack/react-query'
import { useQueryApi } from './rpc-provider'

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const mutationKeys = useMemo(
    () => ({
      nyanpasuConfig: [
        api.queries.getAppConfig().queryKey,
        api.queries.getSysProxy().queryKey,
        // The proxies snapshot is trimmed to the default latency test URL.
        api.queries.getProxies().queryKey,
      ] as QueryKey[],
      clashConfig: [
        api.queries.clashApiGetVersion().queryKey,
        api.queries.getClashConfig().queryKey,
        api.queries.getClashInfo().queryKey,
        api.queries.clashApiGetConfigs().queryKey,
        api.queries.getProfiles().queryKey,
      ] as QueryKey[],
      profiles: [
        api.queries.clashApiGetVersion().queryKey,
        api.queries.getClashConfig().queryKey,
        api.queries.getClashInfo().queryKey,
        api.queries.getProfiles().queryKey,
      ] as QueryKey[],
      proxies: [
        api.queries.getProxies().queryKey,
        api.queries.clashApiGetProvidersProxies().queryKey,
      ] as QueryKey[],
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
    ]
    return () => {
      unlisteners.forEach((unlisten) =>
        unlisten.then((stop) => stop()).catch(console.error),
      )
    }
  }, [api, queryClient, refetchQueries])

  useEffect(() => {
    let disposed = false
    const stopResync = api.listenResync(() => {
      queryClient.invalidateQueries().catch(console.error)
    })
    let stopMutation: (() => void) | undefined
    api
      .listenMutation((payload) => {
        const keys = mutationKeys[payload as keyof typeof mutationKeys]
        if (keys) refetchQueries(keys)
      })
      .then((stop) => {
        if (disposed) stop()
        else {
          stopMutation = stop
        }
      })
      .catch(console.error)

    return () => {
      disposed = true
      stopResync()
      stopMutation?.()
    }
  }, [api, mutationKeys, queryClient, refetchQueries])

  return children
}

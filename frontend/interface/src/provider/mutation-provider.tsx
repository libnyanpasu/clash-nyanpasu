import { PropsWithChildren, useEffect, useRef } from 'react'
import { useQueryClient, type QueryKey } from '@tanstack/react-query'
import { rpc } from '../ipc/rpc'

const NYANPASU_CONFIG_MUTATION_KEYS: QueryKey[] = [
  rpc.queries.getVergeConfig().queryKey,
  rpc.queries.getSysProxy().queryKey,
  // TODO: proxies hook refetch
  // TODO: profiles hook refetch
]

const CLASH_CONFIG_MUTATION_KEYS: QueryKey[] = [
  rpc.queries.clashApiGetVersion().queryKey,
  rpc.queries.getClashInfo().queryKey,
  rpc.queries.clashApiGetConfigs().queryKey,
  rpc.queries.getProfiles().queryKey,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: profiles hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
]

const PROFILES_MUTATION_KEYS: QueryKey[] = [
  rpc.queries.clashApiGetVersion().queryKey,
  rpc.queries.getClashInfo().queryKey,
  rpc.queries.getProfiles().queryKey,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
]

const PROXIES_MUTATION_KEYS: QueryKey[] = [
  rpc.queries.getProxies().queryKey,
  rpc.queries.clashApiGetProvidersProxies().queryKey,
]

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const unlistenFn = useRef<(() => void) | null>(null)

  const queryClient = useQueryClient()

  const refetchQueries = (keys: readonly QueryKey[]) => {
    Promise.all(
      keys.map((queryKey) =>
        queryClient.refetchQueries({
          queryKey,
        }),
      ),
    ).catch((e) => console.error(e))
  }

  useEffect(() => {
    let disposed = false
    const stopResync = rpc.listenResync(() => {
      queryClient.invalidateQueries().catch(console.error)
    })

    rpc
      .listenMutation((payload) => {
        console.log('MutationProvider', payload)

        switch (payload) {
          case 'nyanpasu_config':
            refetchQueries(NYANPASU_CONFIG_MUTATION_KEYS)
            break
          case 'clash_config':
            refetchQueries(CLASH_CONFIG_MUTATION_KEYS)
            break
          case 'profiles':
            refetchQueries(PROFILES_MUTATION_KEYS)
            break
          case 'proxies':
            refetchQueries(PROXIES_MUTATION_KEYS)
            break
        }
      })
      .then((unlisten) => {
        if (disposed) {
          unlisten()
          return
        }

        unlistenFn.current = unlisten
      })
      .catch((e) => {
        console.error(e)
      })

    return () => {
      disposed = true
      stopResync()
      unlistenFn.current?.()
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [])

  return children
}

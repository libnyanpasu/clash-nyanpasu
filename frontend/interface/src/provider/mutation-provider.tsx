import { PropsWithChildren, useEffect, useRef } from 'react'
import { useQueryClient, type QueryKey } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { queries } from '../ipc/bindings'
import { NYANPASU_BACKEND_EVENT_NAME } from '../ipc/consts'

type EventPayload = 'nyanpasu_config' | 'clash_config' | 'proxies' | 'profiles'

const NYANPASU_CONFIG_MUTATION_KEYS: QueryKey[] = [
  queries.getVergeConfig().queryKey,
  queries.getSysProxy().queryKey,
  // TODO: proxies hook refetch
  // TODO: profiles hook refetch
]

const CLASH_CONFIG_MUTATION_KEYS: QueryKey[] = [
  queries.clashApiGetVersion().queryKey,
  queries.getClashInfo().queryKey,
  queries.clashApiGetConfigs().queryKey,
  queries.getProfiles().queryKey,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: profiles hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
]

const PROFILES_MUTATION_KEYS: QueryKey[] = [
  queries.clashApiGetVersion().queryKey,
  queries.getClashInfo().queryKey,
  queries.getProfiles().queryKey,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
]

const PROXIES_MUTATION_KEYS: QueryKey[] = [
  queries.getProxies().queryKey,
  queries.clashApiGetProvidersProxies().queryKey,
]

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const unlistenFn = useRef<UnlistenFn>(null)

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

    listen<EventPayload>(NYANPASU_BACKEND_EVENT_NAME, ({ payload }) => {
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
      unlistenFn.current?.()
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [])

  return children
}

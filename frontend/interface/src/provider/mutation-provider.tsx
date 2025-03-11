import { useMount } from 'ahooks'
import { PropsWithChildren, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  CLASH_CONFIG_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
  NYANPASU_BACKEND_EVENT_NAME,
  NYANPASU_SETTING_QUERY_KEY,
  NYANPASU_SYSTEM_PROXY_QUERY_KEY,
} from '../ipc/consts'

type EventPayload = 'nyanpasu_config' | 'clash_config' | 'proxies' | 'profiles'

const NYANPASU_CONFIG_MUTATION_KEYS = [
  NYANPASU_SETTING_QUERY_KEY,
  NYANPASU_SYSTEM_PROXY_QUERY_KEY,
  // TODO: proxies hook refetch
  // TODO: profiles hook refetch
] as const

const CLASH_CONFIG_MUTATION_KEYS = [
  CLASH_VERSION_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  CLASH_CONFIG_QUERY_KEY,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: profiles hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
] as const

const PROFILES_MUTATION_KEYS = [
  CLASH_VERSION_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  // TODO: clash rules hook refetch
  // TODO: clash rules providers hook refetch
  // TODO: proxies hook refetch
  // TODO: proxies providers hook refetch
  // TODO: profiles hook refetch
  // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
]

const PROXIES_MUTATION_KEYS = [
  // TODO: key.includes('getProxies')
] as const

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const unlistenFn = useRef<UnlistenFn>(null)

  const queryClient = useQueryClient()

  const refetchQueries = (keys: readonly string[]) => {
    Promise.all(
      keys.map((key) =>
        queryClient.refetchQueries({
          queryKey: [key],
        }),
      ),
    ).catch((e) => console.error(e))
  }

  useMount(() => {
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
        unlistenFn.current = unlisten
      })
      .catch((e) => {
        console.error(e)
      })
  })

  return children
}

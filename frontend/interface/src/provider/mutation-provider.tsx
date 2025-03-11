import { useMount } from 'ahooks'
import { PropsWithChildren, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  NYANPASU_BACKEND_EVENT_NAME,
  useClashConfig,
  useClashInfo,
  useClashVersion,
  useSettings,
  useSystemProxy,
} from '../ipc'

type EventPayload = 'nyanpasu_config' | 'clash_config' | 'proxies' | 'profiles'

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const unlistenFn = useRef<UnlistenFn>(null)

  const settings = useSettings()

  const systemProxy = useSystemProxy()

  const clashVersion = useClashVersion()

  const clashInfo = useClashInfo()

  const clashConfig = useClashConfig()

  useMount(() => {
    listen<EventPayload>(NYANPASU_BACKEND_EVENT_NAME, ({ payload }) => {
      switch (payload) {
        case 'nyanpasu_config': {
          Promise.all([
            settings.query.refetch(),
            systemProxy.refetch(),
            // TODO: proxies hook refetch
            // TODO: profiles hook refetch
          ]).catch((e) => {
            console.error(e)
          })

          break
        }

        case 'clash_config': {
          Promise.all([
            clashVersion.refetch(),
            clashInfo.refetch(),
            clashConfig.query.refetch(),
            // TODO: clash rules hook refetch
            // TODO: clash rules providers hook refetch
            // TODO: proxies hook refetch
            // TODO: proxies providers hook refetch
            // TODO: profiles hook refetch
            // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
          ]).catch((e) => {
            console.error(e)
          })

          break
        }

        case 'profiles': {
          Promise.all([
            clashVersion.refetch(),
            clashInfo.refetch(),
            // TODO: clash rules hook refetch
            // TODO: clash rules providers hook refetch
            // TODO: proxies hook refetch
            // TODO: proxies providers hook refetch
            // TODO: profiles hook refetch
            // TODO: all profiles providers hook refetch, key.includes('getAllProxiesProviders')
          ]).catch((e) => {
            console.error(e)
          })

          break
        }

        case 'proxies': {
          Promise.all([
            // TODO: key.includes('getProxies')
          ]).catch((e) => {
            console.error(e)
          })

          break
        }
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

import { useWebSocket } from 'ahooks'
import { useCallback, useMemo } from 'react'
import { useClashInfo } from './use-clash-info'

export const useClashWebSocket = () => {
  const { data: info } = useClashInfo()

  const wsBaseUrl = useMemo(() => `ws://${info?.server}`, [info?.server])

  const tokenParams = useMemo(
    // must have token=, otherwise clash will return 403
    () => `token=${encodeURIComponent(info?.secret || '')}`,
    [info?.secret],
  )

  const resolveUrl = useCallback(
    (path: string) => {
      return `${wsBaseUrl}/${path}?${tokenParams}`
    },
    [wsBaseUrl, tokenParams],
  )

  const urls = useMemo(() => {
    if (info) {
      return {
        connections: resolveUrl('connections'),
        logs: resolveUrl('logs'),
        traffic: resolveUrl('traffic'),
        memory: resolveUrl('memory'),
      }
    }
  }, [info, resolveUrl])

  const connectionsWS = useWebSocket(urls?.connections ?? '')

  const logsWS = useWebSocket(urls?.logs ?? '')

  const trafficWS = useWebSocket(urls?.traffic ?? '')

  const memoryWS = useWebSocket(urls?.memory ?? '')

  return {
    connectionsWS,
    logsWS,
    trafficWS,
    memoryWS,
  }
}

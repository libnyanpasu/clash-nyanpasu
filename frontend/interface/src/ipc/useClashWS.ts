import { useWebSocket } from 'ahooks'
import { useCallback, useMemo } from 'react'
import { useClash } from './useClash'

export const useClashWS = () => {
  const { getClashInfo } = useClash()

  const getBaseUrl = useCallback(() => {
    return `ws://${getClashInfo.data?.server}`
  }, [getClashInfo.data?.server])

  const getTokenUrl = useCallback(() => {
    return `token=${encodeURIComponent(getClashInfo.data?.secret || '')}`
  }, [getClashInfo.data?.secret])

  const resolveUrl = useCallback(
    (path: string) => {
      return `${getBaseUrl()}/${path}?${getTokenUrl()}`
    },
    [getBaseUrl, getTokenUrl],
  )

  const url = useMemo(() => {
    if (getClashInfo.data) {
      return {
        connections: resolveUrl('connections'),
        logs: resolveUrl('logs'),
        traffic: resolveUrl('traffic'),
        memory: resolveUrl('memory'),
      }
    }
  }, [getClashInfo.data, resolveUrl])

  const connections = useWebSocket(url?.connections ?? '')

  const logs = useWebSocket(url?.logs ?? '')

  const traffic = useWebSocket(url?.traffic ?? '')

  const memory = useWebSocket(url?.memory ?? '')

  return {
    connections,
    logs,
    traffic,
    memory,
  }
}

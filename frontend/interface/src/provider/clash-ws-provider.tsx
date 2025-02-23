import { useUpdateEffect } from 'ahooks'
import { PropsWithChildren } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { ClashConnection } from '../ipc/use-clash-connections'
import { ClashMemory } from '../ipc/use-clash-memory'
import { ClashTraffic } from '../ipc/use-clash-traffic'
import { useClashWebSocket } from '../ipc/use-clash-web-socket'

const MAX_CONNECTIONS_HISTORY = 32

const MAX_MEMORY_HISTORY = 32

const MAX_TRAFFIC_HISTORY = 32

export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  const { connectionsWS, memoryWS, trafficWS } = useClashWebSocket()

  const queryClient = useQueryClient()

  // clash connections
  useUpdateEffect(() => {
    const data = JSON.parse(
      connectionsWS.latestMessage?.data,
    ) as ClashConnection

    const currentData = queryClient.getQueryData([
      'clash-connections',
    ]) as ClashConnection[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_CONNECTIONS_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-connections'], newData)
  }, [connectionsWS.latestMessage])

  // clash memory
  useUpdateEffect(() => {
    const data = JSON.parse(memoryWS.latestMessage?.data) as ClashMemory

    const currentData = queryClient.getQueryData([
      'clash-memory',
    ]) as ClashMemory[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_MEMORY_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-memory'], newData)
  }, [memoryWS.latestMessage])

  // clash traffic
  useUpdateEffect(() => {
    const data = JSON.parse(trafficWS.latestMessage?.data) as ClashTraffic

    const currentData = queryClient.getQueryData([
      'clash-traffic',
    ]) as ClashTraffic[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_TRAFFIC_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-traffic'], newData)
  }, [trafficWS.latestMessage])

  return children
}

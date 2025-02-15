import { useUpdateEffect } from 'ahooks'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashWebSocket } from './use-clash-web-socket'

const MAX_MEMORY_HISTORY = 32

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const { trafficWS } = useClashWebSocket()

  const queryClient = useQueryClient()

  useUpdateEffect(() => {
    const data = JSON.parse(trafficWS.latestMessage?.data) as ClashMemory

    const currentData = queryClient.getQueryData([
      'clash-memory',
    ]) as ClashMemory[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_MEMORY_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-memory'], newData)
  }, [trafficWS.latestMessage])

  const query = useQuery<ClashMemory[]>({
    queryKey: ['clash-memory'],
    queryFn: () => [],
  })

  return query
}

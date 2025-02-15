import { useUpdateEffect } from 'ahooks'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashWebSocket } from './use-clash-web-socket'

const MAX_TRAFFIC_HISTORY = 32

export type ClashTraffic = {
  up: number
  down: number
}

export const useClashTraffic = () => {
  const { trafficWS } = useClashWebSocket()

  const queryClient = useQueryClient()

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

  const query = useQuery<ClashTraffic[]>({
    queryKey: ['clash-traffic'],
    queryFn: () => [],
  })

  return query
}

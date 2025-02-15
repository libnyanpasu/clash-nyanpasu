import { useUpdateEffect } from 'ahooks'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useClashWebSocket } from './use-clash-web-socket'

const MAX_LOGS_HISTORY = 1024

export type ClashLog = {
  type: string
  time?: string
  payload: string
}

export const useClashLogs = () => {
  const { logsWS } = useClashWebSocket()

  const queryClient = useQueryClient()

  useUpdateEffect(() => {
    const data = JSON.parse(logsWS.latestMessage?.data) as ClashLog

    const currentData = queryClient.getQueryData(['clash-logs']) as ClashLog[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_LOGS_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData(['clash-logs'], newData)
  }, [logsWS.latestMessage])

  const query = useQuery<ClashLog[]>({
    queryKey: ['clash-logs'],
    queryFn: () => [],
  })

  return query
}

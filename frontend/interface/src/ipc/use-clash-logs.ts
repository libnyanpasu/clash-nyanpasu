import { useMemoizedFn } from 'ahooks'
import { useClashWSContext } from '@/provider/clash-ws-provider'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { CLASH_LOGS_QUERY_KEY } from './consts'

export type ClashLog = {
  type: string
  time?: string
  payload: string
}

export const useClashLogs = () => {
  const { recordLogs, setRecordLogs } = useClashWSContext()

  const queryClient = useQueryClient()

  const query = useQuery<ClashLog[]>({
    queryKey: [CLASH_LOGS_QUERY_KEY],
    queryFn: () => {
      return queryClient.getQueryData<ClashLog[]>([CLASH_LOGS_QUERY_KEY]) || []
    },
  })

  const clean = useMutation({
    mutationFn: async () => {
      await queryClient.setQueryData([CLASH_LOGS_QUERY_KEY], [])
    },
  })

  const status = recordLogs

  const enable = useMemoizedFn(() => {
    setRecordLogs(true)
  })

  const disable = useMemoizedFn(() => {
    setRecordLogs(false)
  })

  return {
    query,
    clean,
    status,
    enable,
    disable,
  }
}

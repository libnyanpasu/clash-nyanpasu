import {
  useClashWSHistory,
  useClashWSStatus,
} from '../provider/clash-ws-provider'

export type ClashLog = {
  type: string
  time?: string
  payload: string
}

export const useClashLogs = () => {
  const logs = useClashWSHistory('logs')
  const { isLoading, error, clearHistory } = useClashWSStatus()

  const clean = {
    mutateAsync: () => clearHistory('logs'),
  }

  return {
    data: logs,
    isLoading,
    error,
    clean,
  }
}

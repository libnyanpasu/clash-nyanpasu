import {
  useClashWSHistory,
  useClashWSStatus,
} from '../provider/clash-ws-provider'

export type ClashTraffic = {
  up: number
  down: number
}

export const useClashTraffic = () => {
  const traffic = useClashWSHistory('traffic')
  const { isLoading, error } = useClashWSStatus()

  return {
    data: traffic,
    isLoading,
    error,
  }
}

import { useClashWSContext } from '@interface/provider/clash-ws-provider'

export type ClashTraffic = {
  up: number
  down: number
}

export const useClashTraffic = () => {
  const { traffic, isLoading, error } = useClashWSContext()

  return {
    data: traffic,
    isLoading,
    error,
  }
}

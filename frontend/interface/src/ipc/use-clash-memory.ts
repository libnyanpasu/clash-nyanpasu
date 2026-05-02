import { useClashWSContext } from '@interface/provider/clash-ws-provider'

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const { memory, isLoading, error } = useClashWSContext()

  return {
    data: memory,
    isLoading,
    error,
  }
}

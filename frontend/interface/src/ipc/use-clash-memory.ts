import {
  useClashWSHistory,
  useClashWSStatus,
} from '@interface/provider/clash-ws-provider'

export type ClashMemory = {
  inuse: number
  oslimit: number
}

export const useClashMemory = () => {
  const memory = useClashWSHistory('memory')
  const { isLoading, error } = useClashWSStatus()

  return {
    data: memory,
    isLoading,
    error,
  }
}

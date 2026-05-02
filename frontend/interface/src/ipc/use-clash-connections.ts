import { useState } from 'react'
import { useClashWSContext } from '@interface/provider/clash-ws-provider'
import { useClashAPI } from '../service/clash-api'

export type ClashConnection = {
  downloadTotal: number
  uploadTotal: number
  downloadSpeed: number
  uploadSpeed: number
  memory?: number
  connections?: ClashConnectionItem[]
}

export type ClashConnectionItem = {
  id: string
  metadata: ClashConnectionMetadata
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
}

export type ClashConnectionMetadata = {
  network: string
  type: string
  host: string
  sourceIP: string
  sourcePort: string
  destinationPort: string
  destinationIP?: string
  destinationIPASN?: string
  process?: string
  processPath?: string
  dnsMode?: string
  dscp?: number
  inboundIP?: string
  inboundName?: string
  inboundPort?: string
  inboundUser?: string
  remoteDestination?: string
  sniffHost?: string
  specialProxy?: string
  specialRules?: string
}

export const useClashConnections = () => {
  const { connections, isLoading, error } = useClashWSContext()
  const clashApi = useClashAPI()
  const [deleteError, setDeleteError] = useState<unknown>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const deleteConnections = {
    isPending: isDeleting,
    error: deleteError,
    mutateAsync: async (id?: string | null) => {
      setIsDeleting(true)
      setDeleteError(null)

      try {
        await clashApi.deleteConnections(id || undefined)
      } catch (error) {
        setDeleteError(error)
        throw error
      } finally {
        setIsDeleting(false)
      }
    },
  }

  return {
    data: connections,
    isLoading,
    error,
    deleteConnections,
  }
}

import { useState } from 'react'
import {
  useClashWSHistory,
  useClashWSStatus,
} from '@interface/provider/clash-ws-provider'
import { unwrapResult } from '../utils'
import { mutations } from './bindings'
import { invokeMutation } from './query-options'

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
  sourceGeoIP?: string[] | null
  destinationGeoIP?: string[] | null
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

// Deleting does not read the connection history, so callers that only close
// connections do not re-render on every connection sample.
export const useDeleteClashConnections = () => {
  const deleteConnectionsCommand = mutations.clashApiDeleteConnections
  const [deleteError, setDeleteError] = useState<unknown>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  return {
    mutationKey: deleteConnectionsCommand.mutationKey,
    isPending: isDeleting,
    error: deleteError,
    mutateAsync: async (id?: string | null) => {
      setIsDeleting(true)
      setDeleteError(null)

      try {
        unwrapResult(
          await invokeMutation(deleteConnectionsCommand, [id ?? null]),
        )
      } catch (error) {
        setDeleteError(error)
        throw error
      } finally {
        setIsDeleting(false)
      }
    },
  }
}

export const useClashConnections = () => {
  const connections = useClashWSHistory('connections')
  const { isLoading, error } = useClashWSStatus()

  return {
    data: connections,
    isLoading,
    error,
  }
}

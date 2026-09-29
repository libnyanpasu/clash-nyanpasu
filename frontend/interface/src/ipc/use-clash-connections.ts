import { useState } from 'react'
import {
  useClashWSHistory,
  useClashWSStatus,
} from '@interface/provider/clash-ws-provider'
import { unwrapResult } from '../utils'
import { mutations } from './bindings'
import { invokeMutation } from './query-options'

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

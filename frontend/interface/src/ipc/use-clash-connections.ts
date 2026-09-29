import { useState } from 'react'
import {
  useClashWSHistory,
  useClashWSStatus,
} from '@interface/provider/clash-ws-provider'
import { unwrapResult } from '../utils'
import { mutations, type ClashWsConnectionSnapshot } from './bindings'
import { invokeMutation } from './query-options'

// The generated `ClashWsConnectionSnapshot` (its `connections` field is
// still `any`: Rust's own specta TODO on that field, not ours) — this alias
// just names what it is: one connection-stream sample's totals, rates and
// per-chain-member rate table. It becomes the summary type once A4 drops
// the raw `connections` list from the wire shape.
export type ClashConnectionsSummary = ClashWsConnectionSnapshot

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

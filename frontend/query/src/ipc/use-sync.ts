import { useEffect } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ConflictChoice,
  SyncConflict,
  SyncSecrets,
  SyncSettings,
  SyncSettingsView,
  SyncStatus,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const SYNC_STATUS_QUERY_KEY = ['getSyncStatus'] as const
export const SYNC_SETTINGS_QUERY_KEY = ['getSyncSettings'] as const
export const SYNC_CONFLICTS_QUERY_KEY = ['listSyncConflicts'] as const
export const SYNC_SNAPSHOTS_QUERY_KEY = ['listSyncSnapshots'] as const

/** Keeps the newer of two statuses; a delayed read never replaces a newer one. */
export function acceptSyncStatus(
  previous: SyncStatus | undefined,
  next: SyncStatus,
) {
  return previous && previous.revision >= next.revision ? previous : next
}

/**
 * The desktop sync surface: status, settings, conflicts and snapshots.
 * Every operation is a desktop-only command, so a browser surface disables it.
 */
export function useSync({ enabled = true }: { enabled?: boolean } = {}) {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const status = useQuery({
    queryKey: SYNC_STATUS_QUERY_KEY,
    enabled,
    queryFn: async () => {
      const next = unwrapResult(await api.getSyncStatus())
      return acceptSyncStatus(
        queryClient.getQueryData<SyncStatus>(SYNC_STATUS_QUERY_KEY),
        next,
      )
    },
    structuralSharing: (previous, next) =>
      acceptSyncStatus(previous as SyncStatus | undefined, next as SyncStatus),
  })

  const settings = useQuery({
    queryKey: SYNC_SETTINGS_QUERY_KEY,
    enabled,
    queryFn: async () => unwrapResult(await api.getSyncSettings()),
  })

  const conflicts = useQuery({
    queryKey: SYNC_CONFLICTS_QUERY_KEY,
    enabled,
    queryFn: async () => unwrapResult(await api.listSyncConflicts()),
  })

  const snapshots = useQuery({
    queryKey: SYNC_SNAPSHOTS_QUERY_KEY,
    enabled,
    queryFn: async () => await api.listSyncSnapshots(),
  })

  useEffect(() => {
    if (!enabled) return
    let disposed = false
    let unlisten: (() => void) | undefined
    const refresh = () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: SYNC_CONFLICTS_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: SYNC_SNAPSHOTS_QUERY_KEY }),
      ]).catch((error: unknown) =>
        console.error('failed to refresh sync state:', error),
      )
    api.events.syncStatusChangedEvent
      .listen(({ payload }) => {
        if (disposed) return
        queryClient.setQueryData<SyncStatus>(
          SYNC_STATUS_QUERY_KEY,
          (previous) => acceptSyncStatus(previous, payload),
        )
        refresh().catch(() => undefined)
      })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
        if (!disposed)
          return queryClient.invalidateQueries({
            queryKey: SYNC_STATUS_QUERY_KEY,
          })
      })
      .catch((error: unknown) => {
        if (!disposed)
          console.error('failed to subscribe to sync status:', error)
      })
    const stopResync = api.listenResync(() => {
      queryClient
        .invalidateQueries({ queryKey: SYNC_STATUS_QUERY_KEY })
        .catch((error: unknown) =>
          console.error('failed to resync sync status:', error),
        )
    })
    return () => {
      disposed = true
      stopResync()
      unlisten?.()
    }
  }, [api, enabled, queryClient])

  const requireEnabled = () => {
    if (!enabled) throw new Error('Configuration sync requires the desktop app')
  }
  const refreshAll = () =>
    Promise.all(
      [
        SYNC_STATUS_QUERY_KEY,
        SYNC_SETTINGS_QUERY_KEY,
        SYNC_CONFLICTS_QUERY_KEY,
        SYNC_SNAPSHOTS_QUERY_KEY,
      ].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
    )

  const configure = useMutation({
    mutationFn: async (input: {
      settings: SyncSettings
      secrets: SyncSecrets | null
    }) => {
      requireEnabled()
      return unwrapResult(
        await api.configureSync(input.settings, input.secrets),
      )
    },
    onSettled: refreshAll,
  })

  const testConnection = useMutation({
    mutationFn: async (input: {
      settings: SyncSettings
      secrets: SyncSecrets | null
    }) => {
      requireEnabled()
      return unwrapResult(
        await api.testSyncConnection(input.settings, input.secrets),
      )
    },
  })

  const enable = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.enableSync())
    },
    onSettled: refreshAll,
  })

  const disable = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.disableSync())
    },
    onSettled: refreshAll,
  })

  const syncNow = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.syncNow())
    },
    onSettled: refreshAll,
  })

  const resolveConflict = useMutation({
    mutationFn: async (input: {
      conflictId: string
      choice: ConflictChoice
    }) => {
      requireEnabled()
      return unwrapResult(
        await api.resolveSyncConflict(input.conflictId, input.choice),
      )
    },
    onSettled: refreshAll,
  })

  const restoreSnapshot = useMutation({
    mutationFn: async (snapshotId: string) => {
      requireEnabled()
      return unwrapResult(await api.restoreSyncSnapshot(snapshotId))
    },
    onSettled: refreshAll,
  })

  return {
    status,
    settings,
    conflicts,
    snapshots,
    configure,
    testConnection,
    enable,
    disable,
    syncNow,
    resolveConflict,
    restoreSnapshot,
  }
}

export type { SyncConflict, SyncSettings, SyncSettingsView }

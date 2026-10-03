import { useEffect } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type { AppUpdateSnapshot } from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

export const APP_UPDATE_QUERY_KEY = ['getAppUpdateState'] as const

export function acceptAppUpdateSnapshot(
  previous: AppUpdateSnapshot | undefined,
  next: AppUpdateSnapshot,
) {
  return previous && previous.revision >= next.revision ? previous : next
}

/** Shares backend-owned update state and operations with application surfaces. */
export function useAppUpdate({ enabled = true }: { enabled?: boolean } = {}) {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: APP_UPDATE_QUERY_KEY,
    enabled,
    queryFn: async () => {
      const next = unwrapResult(await api.getAppUpdateState())
      return acceptAppUpdateSnapshot(
        queryClient.getQueryData<AppUpdateSnapshot>(APP_UPDATE_QUERY_KEY),
        next,
      )
    },
    structuralSharing: (previous, next) =>
      acceptAppUpdateSnapshot(
        previous as AppUpdateSnapshot | undefined,
        next as AppUpdateSnapshot,
      ),
  })

  useEffect(() => {
    if (!enabled) return
    let disposed = false
    let unlisten: (() => void) | undefined
    api.events.appUpdateStateChanged
      .listen(({ payload }) => {
        if (disposed) return
        queryClient.setQueryData<AppUpdateSnapshot>(
          APP_UPDATE_QUERY_KEY,
          (previous) => acceptAppUpdateSnapshot(previous, payload),
        )
      })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
        if (!disposed)
          return queryClient.invalidateQueries({
            queryKey: APP_UPDATE_QUERY_KEY,
          })
      })
      .catch((error: unknown) => {
        if (!disposed)
          console.error(
            'failed to subscribe to application update state:',
            error,
          )
      })
    const stopResync = api.listenResync(() => {
      queryClient
        .invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY })
        .catch((error: unknown) =>
          console.error('failed to resync application update state:', error),
        )
    })

    return () => {
      disposed = true
      stopResync()
      unlisten?.()
    }
  }, [api, enabled, queryClient])

  const requireEnabled = () => {
    if (!enabled) throw new Error('Application updates require the desktop app')
  }

  const check = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.checkAppUpdate())
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY }),
  })
  const download = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.downloadAppUpdate())
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY }),
  })
  const cancelDownload = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.cancelAppUpdateDownload())
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY }),
  })
  const install = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.installAppUpdate())
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY }),
  })
  const discardPackage = useMutation({
    mutationFn: async () => {
      requireEnabled()
      return unwrapResult(await api.discardAppUpdatePackage())
    },
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: APP_UPDATE_QUERY_KEY }),
  })

  return {
    query,
    snapshot: query.data,
    isLoading: query.isLoading,
    isPending:
      check.isPending ||
      download.isPending ||
      cancelDownload.isPending ||
      install.isPending ||
      discardPackage.isPending,
    refresh: query.refetch,
    check: check.mutateAsync,
    download: download.mutateAsync,
    cancelDownload: cancelDownload.mutateAsync,
    install: install.mutateAsync,
    discardPackage: discardPackage.mutateAsync,
  }
}

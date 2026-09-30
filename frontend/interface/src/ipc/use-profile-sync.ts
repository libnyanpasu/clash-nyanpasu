import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, queries, type RunCursorDto } from './bindings'

export function useProfileSyncStatus(uid: string) {
  const options = queries.getProfileSyncStatus(uid)
  return useQuery({
    queryKey: options.queryKey,
    queryFn: async () => unwrapResult(await commands.getProfileSyncStatus(uid)),
    refetchInterval: 2000,
  })
}

export function useProfileSyncRuns(uid: string) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-runs', uid],
    initialPageParam: null as RunCursorDto | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncRuns(uid, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
    refetchInterval: 2000,
  })
}

export function useProfileSyncLogs(uid: string, run: string | null) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-logs', uid, run],
    initialPageParam: null as string | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncLogs(uid, run!, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
    enabled: run !== null,
    refetchInterval: 2000,
  })
}

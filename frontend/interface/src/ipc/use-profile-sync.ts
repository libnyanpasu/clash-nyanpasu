import {
  useInfiniteQuery,
  useIsMutating,
  useQuery,
} from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  commands,
  mutations,
  queries,
  type ProfileSyncStatus,
  type RunCursorDto,
} from './rpc-bindings'

const LIVE_INTERVAL = 2000
// Catches runs started elsewhere (another window, a catch-up run) while idle.
const IDLE_INTERVAL = 60_000

const statusInterval = (
  status: ProfileSyncStatus | undefined,
  refreshing: boolean,
) => {
  if (refreshing || (status?.active.length ?? 0) > 0) return LIVE_INTERVAL
  if (!status?.next_run_at) return IDLE_INTERVAL
  // Wake up just after the scheduled run is admitted.
  const untilNextRun = Date.parse(status.next_run_at) - Date.now() + 1000
  return Math.min(Math.max(untilNextRun, LIVE_INTERVAL), IDLE_INTERVAL)
}

/**
 * Polls quickly only while a run is active or a manual refresh of this
 * profile (which runs the sync job) is in flight.
 */
export function useProfileSyncStatus(uid: string) {
  const options = queries.getProfileSyncStatus(uid)
  const refreshing =
    useIsMutating({
      mutationKey: mutations.updateProfile.mutationKey,
      predicate: (mutation) =>
        (mutation.state.variables as { uid?: string } | undefined)?.uid === uid,
    }) > 0
  return useQuery({
    queryKey: options.queryKey,
    queryFn: async () => unwrapResult(await commands.getProfileSyncStatus(uid)),
    refetchInterval: (query) => statusInterval(query.state.data, refreshing),
  })
}

/** Not polled: refresh it (key `['profile-sync-runs', uid]`) when runs change. */
export function useProfileSyncRuns(uid: string) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-runs', uid],
    initialPageParam: null as RunCursorDto | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncRuns(uid, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
  })
}

/** A finished run's logs never change, so only a live run's logs are polled. */
export function useProfileSyncLogs(
  uid: string,
  run: string | null,
  live: boolean,
) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-logs', uid, run],
    initialPageParam: null as string | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncLogs(uid, run!, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
    enabled: run !== null,
    refetchInterval: live ? LIVE_INTERVAL : false,
  })
}

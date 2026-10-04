import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type {
  CoreLogCursor,
  CoreLogError,
  CoreLogRow,
  CoreLogStatus,
} from '@nyanpasu/rpc/types'
import { useRpc } from '../provider/rpc-provider'
import { mergeCoreLogRows } from './core-log-viewer-state'

export type ClashLog = CoreLogRow
type View = {
  data: CoreLogRow[]
  status: CoreLogStatus | null
  error: CoreLogError | null
  isLoading: boolean
  loadingOlder: boolean
  more: boolean
}
const empty = (): View => ({
  data: [],
  status: null,
  error: null,
  isLoading: true,
  loadingOlder: false,
  more: false,
})
const failure = (error: unknown): CoreLogError =>
  typeof error === 'object' && error !== null && 'kind' in error
    ? (error as CoreLogError)
    : { kind: 'unavailable', message: String(error) }

/** Page-local previews. Neither the query cache nor the global WS provider owns bodies. */
export function useClashLogs(
  level: string | null = null,
  keyword = '',
  following = true,
) {
  const rpc = useRpc()
  const [view, setView] = useState<View>(empty)
  const controls = useRef({
    older: () => {},
    clear: async () => {},
    retry: () => {},
  })
  const live = useRef(following)
  live.current = following

  useEffect(() => {
    let disposed = false
    let ready = false
    let busy = false
    let wake = false
    let epoch = 0
    let loaded = false
    let needsStatus = document.hidden
    let status: CoreLogStatus | null = null
    let tail: CoreLogCursor | null = null
    let older: CoreLogCursor | null = null
    let requestOlder = false
    let more = false
    let rows: CoreLogRow[] = []
    let unlisten: (() => void) | undefined
    setView(empty())

    const reset = () => {
      rows = []
      tail = null
      older = null
      more = false
      requestOlder = false
      loaded = false
    }
    const publish = (error: CoreLogError | null = null) => {
      const next = {
        data: rows,
        status,
        error,
        isLoading: !loaded,
        loadingOlder: requestOlder,
        more,
      }
      setView((value) =>
        value.data === next.data &&
        value.status === next.status &&
        value.error === next.error &&
        value.isLoading === next.isLoading &&
        value.loadingOlder === next.loadingOlder &&
        value.more === next.more
          ? value
          : next,
      )
    }
    const acceptStatus = (next: CoreLogStatus) => {
      if (status && next.version <= status.version) return false
      if (status?.generation !== next.generation || !next.head) {
        reset()
        loaded = next.head === null
      }
      status = next
      rows = mergeCoreLogRows(rows, [], next)
      return true
    }
    const sync = async () => {
      if (disposed || !ready) return
      if (busy) {
        wake = true
        return
      }
      busy = true
      const requestEpoch = epoch
      try {
        if (needsStatus) {
          needsStatus = false
          const next = unwrapResult(await rpc.getCoreLogStatus())
          if (disposed || epoch !== requestEpoch) return
          acceptStatus(next)
          publish()
        }
        if (document.hidden) return
        const hasNew =
          status?.head && (!tail || status.head.sequence > tail.sequence)
        if (loaded && !requestOlder && (!live.current || !hasNew)) return
        const direction =
          requestOlder && older ? 'before' : tail ? 'after' : 'latest'
        const page = unwrapResult(
          await rpc.queryCoreLogs({
            direction,
            cursor: direction === 'before' ? older : tail,
            level,
            keyword,
            limit: 200,
          }),
        )
        if (disposed || epoch !== requestEpoch) return
        acceptStatus(page.status)
        // A clear event can arrive while the old generation's page is in flight.
        if (page.status.generation !== status!.generation) {
          wake = true
          return
        }
        if (direction === 'latest') {
          tail = page.status.head
          older = page.cursor
          more = page.more
        } else if (direction === 'before') {
          older = page.cursor
          more = page.more
          requestOlder = false
        } else {
          tail = page.more ? page.cursor : page.status.head
          if (page.more) wake = true
        }
        rows = mergeCoreLogRows(
          rows,
          page.rows,
          status!,
          direction === 'before',
        )
        loaded = true
        publish()
      } catch (error) {
        if (disposed || epoch !== requestEpoch) return
        const problem = failure(error)
        requestOlder = false
        if (problem.kind === 'cursor_expired') {
          reset()
          wake = true
        } else {
          loaded = true
        }
        publish(problem.kind === 'cursor_expired' ? null : problem)
      } finally {
        busy = false
        if (wake && !disposed) {
          wake = false
          sync()
        }
      }
    }
    const subscribe = async () => {
      try {
        const stop = await rpc.events.coreLogsChanged.listen(({ payload }) => {
          if (disposed || !acceptStatus(payload.status)) return
          publish()
          sync()
        })
        if (disposed) {
          stop()
          return
        }
        unlisten = stop
        ready = true
        sync()
      } catch (error) {
        if (!disposed) {
          loaded = true
          publish(failure(error))
        }
      }
    }
    controls.current = {
      older: () => {
        if (!older || !more || requestOlder) return
        requestOlder = true
        publish()
        sync()
      },
      clear: async () => {
        epoch += 1
        try {
          unwrapResult(await rpc.clearCoreLogs())
          if (disposed) return
          reset()
          needsStatus = true
          publish()
          sync()
        } catch (error) {
          if (!disposed) publish(failure(error))
          throw error
        }
      },
      retry: () => {
        epoch += 1
        reset()
        publish()
        if (ready) sync()
        else subscribe()
      },
    }
    const resync = () => {
      needsStatus = true
      sync()
    }
    const stopResync = rpc.listenResync(resync)
    const resume = () => {
      if (!document.hidden) resync()
    }
    window.addEventListener('focus', resume)
    document.addEventListener('visibilitychange', resume)
    subscribe()

    return () => {
      disposed = true
      unlisten?.()
      stopResync()
      window.removeEventListener('focus', resume)
      document.removeEventListener('visibilitychange', resume)
      controls.current = {
        older: () => {},
        clear: async () => {},
        retry: () => {},
      }
    }
  }, [rpc, level, keyword])

  const wasFollowing = useRef(following)
  useEffect(() => {
    if (following && !wasFollowing.current) controls.current.retry()
    wasFollowing.current = following
  }, [following])

  const detailBusy = useRef(false)
  const detail = useCallback(
    async (cursor: CoreLogCursor) => {
      if (detailBusy.current)
        throw new Error('A Core log detail request is already running')
      detailBusy.current = true
      try {
        return unwrapResult(await rpc.getCoreLog(cursor))
      } finally {
        detailBusy.current = false
      }
    },
    [rpc],
  )
  const clean = useMemo(
    () => ({ mutateAsync: () => controls.current.clear() }),
    [],
  )
  const loadOlder = useCallback(() => controls.current.older(), [])
  const retry = useCallback(() => controls.current.retry(), [])

  return { ...view, clean, loadOlder, retry, detail }
}

import { useCallback, useEffect, useRef, useState } from 'react'
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
    let busy = false
    let wake = false
    let epoch = 0
    let generation: string | null = null
    let tail: CoreLogCursor | null = null
    let older: CoreLogCursor | null = null
    let requestOlder = false
    let more = false
    let rows: CoreLogRow[] = []
    let timer: ReturnType<typeof setTimeout> | undefined
    let unlisten: (() => void) | undefined
    setView(empty())

    const reset = () => {
      rows = []
      tail = null
      older = null
      more = false
      requestOlder = false
    }
    const poll = async () => {
      if (disposed) return
      if (busy) {
        wake = true
        return
      }
      if (timer) clearTimeout(timer)
      busy = true
      const requestEpoch = epoch
      let delay = 1000
      try {
        const status = unwrapResult(await rpc.getCoreLogStatus())
        if (disposed || epoch !== requestEpoch) return
        if (generation !== status.generation) {
          reset()
          generation = status.generation
        }
        rows = mergeCoreLogRows(rows, [], status)
        if (!live.current && !requestOlder && tail) {
          setView((value) => ({ ...value, data: rows, status }))
          return
        }
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
        generation = page.status.generation
        if (direction === 'latest') {
          tail = page.status.head
          older = page.cursor
          more = page.more
        } else if (direction === 'before') {
          older = page.cursor
          more = page.more
          requestOlder = false
        } else {
          tail = page.cursor ?? tail
          if (page.more) delay = 25
        }
        rows = mergeCoreLogRows(
          rows,
          page.rows,
          page.status,
          direction === 'before',
        )
        setView({
          data: rows,
          status: page.status,
          error: null,
          isLoading: false,
          loadingOlder: false,
          more,
        })
      } catch (error) {
        if (disposed || epoch !== requestEpoch) return
        const failure: CoreLogError =
          typeof error === 'object' && error !== null && 'kind' in error
            ? (error as CoreLogError)
            : { kind: 'unavailable', message: String(error) }
        if (failure.kind === 'cursor_expired') {
          reset()
          delay = 25
          setView(empty())
        } else {
          requestOlder = false
          setView((value) => ({
            ...value,
            error: failure,
            isLoading: false,
            loadingOlder: false,
          }))
          delay = 2000
        }
      } finally {
        busy = false
        if (!disposed) {
          if (wake) delay = 25
          wake = false
          timer = setTimeout(() => poll(), delay)
        }
      }
    }
    controls.current = {
      older: () => {
        if (!older || !more || requestOlder) return
        requestOlder = true
        setView((value) => ({ ...value, loadingOlder: true }))
        poll()
      },
      clear: async () => {
        epoch += 1
        try {
          unwrapResult(await rpc.clearCoreLogs())
          if (disposed) return
          reset()
          setView(empty())
          poll()
        } catch (error) {
          if (!disposed)
            setView((value) => ({ ...value, error: error as CoreLogError }))
          throw error
        }
      },
      retry: () => {
        epoch += 1
        reset()
        setView(empty())
        poll()
      },
    }
    rpc.events.coreLogsChanged
      .listen(() => poll())
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch((error) => {
        if (!disposed)
          setView((value) => ({
            ...value,
            error: { kind: 'unavailable', message: String(error) },
          }))
      })
    const stopResync = rpc.listenResync(poll)
    const resume = () => {
      if (!document.hidden) poll()
    }
    window.addEventListener('focus', resume)
    document.addEventListener('visibilitychange', resume)
    poll()

    return () => {
      disposed = true
      if (timer) clearTimeout(timer)
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

  useEffect(() => {
    if (following) controls.current.retry()
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

  return {
    ...view,
    clean: { mutateAsync: () => controls.current.clear() },
    loadOlder: () => controls.current.older(),
    retry: () => controls.current.retry(),
    detail,
  }
}

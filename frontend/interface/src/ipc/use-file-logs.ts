import { useEffect, useRef, useState } from 'react'
import { unwrapResult } from '../utils'
import { advanceCursor, mergeLogRows } from './log-viewer-state'
import { rpc } from './rpc'
import {
  type Filter,
  type LogCursor,
  type LogError,
  type LogFileInfo,
  type LogPage,
  type LogRow,
  type LogSource,
} from './rpc-bindings'

type View = {
  rows: LogRow[]
  files: LogFileInfo[]
  page: LogPage | null
  error: LogError | null
  loadingOlder: boolean
  loading: boolean
  more: boolean
}
const empty = (): View => ({
  rows: [],
  files: [],
  page: null,
  error: null,
  loadingOlder: false,
  loading: true,
  more: false,
})

/** Whether two pages show the same status above the rows. */
const sameStatus = (a: LogPage, b: LogPage) =>
  a.partial === b.partial &&
  a.malformed === b.malformed &&
  a.truncated === b.truncated &&
  a.indexed_bytes === b.indexed_bytes &&
  a.file_bytes === b.file_bytes

/** Mount once per Logs route/source. No global cache retains closed viewer sessions. */
export function useFileLogs(
  source: LogSource,
  file: string | null,
  filter: Filter,
) {
  const [view, setView] = useState<View>(empty)
  const [revision, setRevision] = useState(0)
  const controls = useRef({
    older: () => {},
    clear: () => {},
    latest: () => {},
  })
  const filterKey = JSON.stringify(filter)

  useEffect(() => {
    let disposed = false
    let session: string | null = null
    let tail: LogCursor | null = null
    let older: LogCursor | null = null
    let floor: LogCursor | null = null
    let head: LogCursor | null = null
    let requestOlder = false
    let busy = false
    let failures = 0
    let epoch = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    let rows: LogRow[] = []
    const currentFilter: Filter = JSON.parse(filterKey)
    const close = (id: string) =>
      rpc.closeLogSession(source, id).catch(() => {})
    setView(empty())

    const poll = async () => {
      if (disposed || busy) return
      if (timer) clearTimeout(timer)
      busy = true
      let delay = 1000
      try {
        if (!session) {
          const files = unwrapResult(await rpc.listLogFiles(source))
          if (disposed) return
          setView((value) => ({ ...value, files }))
          if (!files.length) {
            setView((value) => ({ ...value, loading: false, error: null }))
            delay = 1000
            return
          }
          const opened = unwrapResult(
            await rpc.openLogSession(source, {
              request_id: crypto.randomUUID(),
              file,
            }),
          )
          if (disposed) {
            close(opened.id)
            return
          }
          session = opened.id
        }
        const direction =
          requestOlder && older && !floor ? 'before' : tail ? 'after' : 'latest'
        const requestEpoch = epoch
        const page = unwrapResult(
          await rpc.queryLogs(source, {
            session,
            filter: currentFilter,
            direction,
            cursor: direction === 'before' ? older : tail,
            limit: 200,
          }),
        )
        if (disposed) return
        if (requestEpoch !== epoch) return
        failures = 0
        if (page.building) {
          setView((value) => ({ ...value, page, loading: true, error: null }))
          delay = 100
          return
        }
        head = page.head
        if (direction === 'before') {
          older = page.cursor
          requestOlder = false
        } else if (direction === 'latest') {
          tail = advanceCursor(page.head, floor)
          older = page.cursor
        } else {
          tail = advanceCursor(page.cursor, floor)
          if (page.more) delay = 25
        }
        rows = mergeLogRows(rows, page.rows, floor, direction === 'before')
        setView((value) => {
          const more = floor
            ? false
            : direction === 'after'
              ? value.more
              : page.more
          // An idle tail poll brings a new page object and nothing to show;
          // keeping the view spares the viewer a render every second.
          if (
            value.rows === rows &&
            value.page &&
            sameStatus(value.page, page) &&
            !value.loading &&
            value.loadingOlder === requestOlder &&
            !value.error &&
            value.more === more
          )
            return value
          return {
            ...value,
            rows,
            page,
            loading: false,
            loadingOlder: requestOlder,
            error: null,
            more,
          }
        })
      } catch (error) {
        if (disposed) return
        const kind =
          typeof error === 'string' ? (error as LogError) : 'unavailable'
        if (kind === 'session_expired' || kind === 'cursor_reset') {
          if (kind === 'session_expired') session = null
          rows = []
          tail = null
          older = null
          floor = null
          head = null
          requestOlder = false
          setView((value) => ({
            ...value,
            rows,
            page: null,
            loading: true,
            loadingOlder: false,
            error: null,
            more: false,
          }))
          delay = 100
        } else {
          setView((value) => ({
            ...value,
            error: kind,
            loading: false,
            loadingOlder: false,
          }))
          delay = Math.min(15_000, 1000 * 2 ** Math.min(++failures, 4))
          if (
            kind === 'unsupported' ||
            kind === 'file_gone' ||
            kind === 'invalid_request'
          )
            delay = 0
        }
      } finally {
        busy = false
        if (!disposed && delay > 0) timer = setTimeout(() => poll(), delay)
      }
    }
    controls.current = {
      older: () => {
        if (requestOlder || !older || floor) return
        requestOlder = true
        setView((value) => ({ ...value, loadingOlder: true }))
        poll()
      },
      clear: () => {
        if (!head) return
        floor = head
        tail = head
        rows = []
        requestOlder = false
        setView((value) => ({
          ...value,
          rows,
          more: false,
          loadingOlder: false,
        }))
      },
      latest: () => {
        epoch += 1
        tail = null
        older = null
        rows = []
        requestOlder = false
        setView((value) => ({
          ...value,
          rows,
          more: false,
          loadingOlder: false,
        }))
        poll()
      },
    }
    const resume = () => {
      if (!document.hidden) poll()
    }
    document.addEventListener('visibilitychange', resume)
    window.addEventListener('focus', resume)
    poll()
    return () => {
      disposed = true
      if (timer) clearTimeout(timer)
      document.removeEventListener('visibilitychange', resume)
      window.removeEventListener('focus', resume)
      if (session) close(session)
      controls.current = { older: () => {}, clear: () => {}, latest: () => {} }
    }
  }, [source, file, filterKey, revision])

  return {
    ...view,
    loadOlder: () => controls.current.older(),
    clear: () => controls.current.clear(),
    latest: () => controls.current.latest(),
    retry: () => setRevision((value) => value + 1),
  }
}

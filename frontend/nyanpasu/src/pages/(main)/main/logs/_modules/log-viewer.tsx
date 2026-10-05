import ArticleRounded from '~icons/material-symbols/article-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import DataObjectRounded from '~icons/material-symbols/data-object-rounded'
import DeleteSweepRounded from '~icons/material-symbols/delete-sweep-rounded'
import VerticalAlignBottomRounded from '~icons/material-symbols/vertical-align-bottom-rounded'
import { memo, useEffect, useId, useState, type ReactNode } from 'react'
import { Button, buttonVariants } from '@nyanpasu/ui/button'
import HighlightText from '@nyanpasu/ui/highlight-text'
import { m } from '@/paraglide/messages'
import { writeClipboardText } from '@/utils/clipboard'
import { cn } from '@nyanpasu/utils'
import LogJson from './log-json'
import LogLevelBadge from './log-level-badge'

export const logPanelClass = 'relative flex min-h-0 min-w-0 flex-1 flex-col'

export function LogSearch({
  value,
  onChange,
  placeholder,
}: {
  value: string
  onChange: (value: string) => void
  placeholder: string
}) {
  return (
    <input
      type="search"
      className={cn(
        'bg-surface-variant dark:bg-surface-variant/30',
        'h-10 min-w-32 flex-1 rounded-full px-4 text-sm outline-none',
      )}
      data-slot="logs-search"
      aria-label={placeholder}
      placeholder={placeholder}
      value={value}
      maxLength={256}
      onChange={(event) => onChange(event.target.value)}
    />
  )
}

export function LogEmptyState({
  children = m.logs_empty_message(),
}: {
  children?: ReactNode
}) {
  return (
    <div
      className="text-on-surface-variant flex flex-col items-center justify-center gap-4 px-6 py-16 text-center text-sm"
      data-slot="logs-no-logs"
    >
      <span className="bg-secondary-container text-on-secondary-container grid size-16 place-items-center rounded-2xl">
        <ArticleRounded aria-hidden className="size-8" />
      </span>
      <p>{children}</p>
    </div>
  )
}

export function LogClearButton({
  disabled,
  onClick,
  label = m.logs_clear_display(),
}: {
  disabled: boolean
  onClick: () => void
  label?: string
}) {
  return (
    <Button
      icon
      disabled={disabled}
      aria-disabled={disabled}
      onClick={onClick}
      aria-label={label}
      title={label}
      className="focus-visible:ring-primary shrink-0 focus-visible:ring-2"
    >
      <DeleteSweepRounded aria-hidden className="size-5" />
    </Button>
  )
}

export function LogFollowButton({
  onClick,
  unseen = 0,
}: {
  onClick: () => void
  unseen?: number
}) {
  return (
    <Button
      variant="fab"
      icon
      onClick={onClick}
      aria-label={m.logs_follow_latest()}
      title={m.logs_follow_latest()}
      className="focus-visible:ring-primary absolute right-4 bottom-4 z-20 overflow-visible focus-visible:ring-2"
    >
      <VerticalAlignBottomRounded aria-hidden className="size-6" />
      {unseen > 0 && (
        <span className="bg-primary text-on-primary absolute -top-1 -right-1 rounded-full px-2 text-xs tabular-nums">
          {unseen > 99 ? '99+' : unseen}
        </span>
      )}
    </Button>
  )
}

// Every visible row has two action buttons. The shared Button keeps ripple
// state and motion per instance, which outweighs the rest of a row, so rows
// use plain buttons with the same look.
const rowActionClass = cn(
  buttonVariants({ icon: true }),
  'focus-visible:ring-primary shrink-0 focus-visible:ring-2',
)

export const LogRecord = memo(function LogRecord({
  time,
  timeTitle,
  level,
  target,
  message,
  raw,
  search,
  incomplete,
  incompleteTitle,
  loadRaw,
  expanded,
  onInspect,
}: {
  time: string
  timeTitle?: string
  level: string
  target?: string
  message: string
  /** The raw text, or a record to show as JSON when copied or inspected. */
  raw: unknown
  search: string
  incomplete?: boolean
  incompleteTitle?: string
  loadRaw?: () => Promise<unknown>
  expanded?: boolean
  onInspect: () => void
}) {
  const [showJson, setShowJson] = useState(false)
  const showing = expanded ?? showJson
  const [loadedRaw, setLoadedRaw] = useState<string | null>(null)
  const [loadingRaw, setLoadingRaw] = useState(false)
  const [detailError, setDetailError] = useState(false)
  const [copying, setCopying] = useState(false)
  useEffect(() => {
    if (!showing || !loadRaw) {
      setLoadedRaw(null)
      setDetailError(false)
      setLoadingRaw(false)
      return
    }
    let disposed = false
    setLoadingRaw(true)
    setLoadedRaw(null)
    setDetailError(false)
    loadRaw()
      .then(
        (value) => {
          if (!disposed)
            setLoadedRaw(
              typeof value === 'string'
                ? value
                : JSON.stringify(value, null, 2),
            )
        },
        () => {
          if (!disposed) setDetailError(true)
        },
      )
      .finally(() => {
        if (!disposed) setLoadingRaw(false)
      })
    return () => {
      disposed = true
    }
  }, [showing, loadRaw])
  const jsonId = useId()
  const [copied, setCopied] = useState(false)
  const [copyError, setCopyError] = useState(false)
  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 2000)
    return () => clearTimeout(timer)
  }, [copied])
  const rawText = () =>
    loadedRaw ?? (typeof raw === 'string' ? raw : JSON.stringify(raw, null, 2))
  return (
    <article className="group/log-record border-outline-variant/50 hover:bg-on-surface/4 focus-within:bg-on-surface/4 border-b px-3 py-2 text-sm transition-colors sm:px-4">
      <div className="text-on-surface-variant flex min-h-10 flex-wrap items-center gap-x-3 gap-y-1">
        <time title={timeTitle} className="font-mono text-xs tabular-nums">
          <HighlightText searchText={search}>{time || '—'}</HighlightText>
        </time>
        <LogLevelBadge searchText={search}>{level}</LogLevelBadge>
        {target && (
          <span className="min-w-0 font-mono text-xs break-all">
            <HighlightText searchText={search}>{target}</HighlightText>
          </span>
        )}
        <div className="ml-auto flex shrink-0 opacity-0 transition-opacity group-focus-within/log-record:opacity-100 group-hover/log-record:opacity-100 [@media(hover:none)]:opacity-100">
          <button
            type="button"
            className={rowActionClass}
            aria-label={copied ? m.logs_copied() : m.logs_copy()}
            title={copied ? m.logs_copied() : m.logs_copy()}
            disabled={copying || loadingRaw}
            onClick={async () => {
              if (copying) return
              setCopying(true)
              try {
                const value = loadRaw && !loadedRaw ? await loadRaw() : null
                const text =
                  value === null
                    ? rawText()
                    : typeof value === 'string'
                      ? value
                      : JSON.stringify(value, null, 2)
                await writeClipboardText(text)
                setCopied(true)
                setCopyError(false)
              } catch {
                setCopyError(true)
              } finally {
                setCopying(false)
              }
            }}
          >
            {copied ? (
              <CheckRounded aria-hidden className="size-4" />
            ) : (
              <ContentCopyRounded aria-hidden className="size-4" />
            )}
          </button>
          <button
            type="button"
            aria-label={m.logs_view_json()}
            title={m.logs_view_json()}
            aria-expanded={showing}
            aria-controls={jsonId}
            className={rowActionClass}
            onClick={() => {
              if (expanded !== undefined || !showing) onInspect()
              if (expanded === undefined) setShowJson(!showJson)
            }}
          >
            <DataObjectRounded aria-hidden className="size-4" />
          </button>
        </div>
      </div>
      <div className="text-on-surface font-mono leading-relaxed [overflow-wrap:anywhere] break-words whitespace-pre-wrap">
        <HighlightText searchText={search}>{message}</HighlightText>
      </div>
      {incomplete && (
        <span
          className="text-on-surface-variant text-xs"
          title={incompleteTitle ?? m.logs_raw_record()}
        >
          ⚠
        </span>
      )}
      {showing && (
        <div id={jsonId} role="region" aria-label={m.logs_view_json()}>
          {loadingRaw ? (
            <span>{m.logs_loading()}</span>
          ) : detailError ? (
            <p role="alert" className="text-error">
              {m.logs_core_detail_unavailable()}
            </p>
          ) : (
            <LogJson raw={rawText()} />
          )}
        </div>
      )}
      {copyError && (
        <p role="alert" className="text-error mt-2 text-xs">
          {m.logs_copy_failed()}
        </p>
      )}
    </article>
  )
})

import ArticleRounded from '~icons/material-symbols/article-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import DeleteSweepRounded from '~icons/material-symbols/delete-sweep-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import VerticalAlignBottomRounded from '~icons/material-symbols/vertical-align-bottom-rounded'
import {
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from 'react'
import { Button } from '@/components/ui/button'
import HighlightText from '@/components/ui/highlight-text'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import LogLevelBadge from './log-level-badge'

export const logPanelClass =
  'bg-surface text-on-surface mx-3 mb-3 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl'

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
    <div className="shrink-0 px-3 pb-3" data-slot="logs-search">
      <label className="bg-surface-variant/40 text-on-surface-variant focus-within:ring-primary flex h-12 items-center gap-3 rounded-full px-4 focus-within:ring-2">
        <SearchRounded aria-hidden className="size-5 shrink-0" />
        <input
          type="search"
          className="text-on-surface placeholder:text-on-surface-variant min-w-0 flex-1 bg-transparent text-sm outline-none"
          aria-label={placeholder}
          placeholder={placeholder}
          value={value}
          maxLength={256}
          onChange={(event) => onChange(event.target.value)}
        />
      </label>
    </div>
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

export function useLogToolbar() {
  const ref = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(56)
  useLayoutEffect(() => {
    if (!ref.current) return
    let frame = 0
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() =>
        setHeight(ref.current?.offsetHeight ?? 56),
      )
    })
    observer.observe(ref.current)
    return () => {
      observer.disconnect()
      cancelAnimationFrame(frame)
    }
  }, [])
  return { ref, height }
}

export function LogViewerActions({
  ref,
  following,
  unseen = 0,
  onFollow,
  onClear,
  clearDisabled,
  children,
}: {
  ref?: Ref<HTMLDivElement>
  following: boolean
  unseen?: number
  onFollow: () => void
  onClear: () => void
  clearDisabled: boolean
  children?: ReactNode
}) {
  return (
    <div
      ref={ref}
      className="bg-surface border-outline-variant/50 sticky top-0 z-10 flex min-h-14 flex-wrap items-center gap-2 border-b px-2 py-2"
    >
      <Button
        aria-pressed={following}
        onClick={() => onFollow()}
        className={cn(
          'focus-visible:ring-primary flex items-center gap-2 focus-visible:ring-2',
          following &&
            'bg-secondary-container text-on-secondary-container dark:bg-secondary-container dark:text-on-secondary-container',
        )}
      >
        <VerticalAlignBottomRounded aria-hidden className="size-5" />
        {m.logs_follow_latest()}
        {unseen > 0 && (
          <span className="bg-primary text-on-primary rounded-full px-2 text-xs tabular-nums">
            {unseen}
          </span>
        )}
      </Button>
      {children}
      <Button
        className="focus-visible:ring-primary ml-auto flex w-10 min-w-10 items-center justify-center gap-2 p-0 focus-visible:ring-2 sm:w-auto sm:px-4"
        disabled={clearDisabled}
        aria-disabled={clearDisabled}
        aria-label={m.logs_clear_display()}
        title={m.logs_clear_display()}
        onClick={() => onClear()}
      >
        <DeleteSweepRounded aria-hidden className="size-5" />
        <span className="hidden sm:inline">{m.logs_clear_display()}</span>
      </Button>
    </div>
  )
}

export function LogRecord({
  time,
  timeTitle,
  level,
  target,
  message,
  raw,
  search,
  incomplete,
  onInspect,
}: {
  time: string
  timeTitle?: string
  level: string
  target?: string
  message: string
  raw: string
  search: string
  incomplete?: boolean
  onInspect: () => void
}) {
  const [copied, setCopied] = useState(false)
  const [copyError, setCopyError] = useState(false)
  return (
    <article className="border-outline-variant/50 hover:bg-on-surface/4 focus-within:bg-on-surface/4 border-b px-3 py-2 text-sm transition-colors sm:px-4">
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
        <Button
          icon
          className="focus-visible:ring-primary ml-auto shrink-0 focus-visible:ring-2"
          aria-label={copied ? m.logs_copied() : m.logs_copy()}
          title={copied ? m.logs_copied() : m.logs_copy()}
          onClick={() =>
            navigator.clipboard.writeText(raw).then(
              () => {
                setCopied(true)
                setCopyError(false)
              },
              () => setCopyError(true),
            )
          }
        >
          {copied ? (
            <CheckRounded aria-hidden className="size-4" />
          ) : (
            <ContentCopyRounded aria-hidden className="size-4" />
          )}
        </Button>
      </div>
      <div className="text-on-surface font-mono leading-relaxed [overflow-wrap:anywhere] break-words whitespace-pre-wrap">
        <HighlightText searchText={search}>{message}</HighlightText>
      </div>
      <details
        className="group mt-2 text-xs"
        onToggle={(event) => {
          if (event.currentTarget.open) onInspect()
        }}
      >
        <summary className="text-on-surface-variant hover:text-primary focus-visible:outline-primary w-fit cursor-pointer rounded py-1 focus-visible:outline-2">
          {m.logs_raw_record()}
          {incomplete && ' ⚠'}
        </summary>
        <pre className="bg-surface-variant/30 text-on-surface mt-2 rounded-xl p-3 font-mono leading-relaxed [overflow-wrap:anywhere] break-words whitespace-pre-wrap">
          {raw}
        </pre>
      </details>
      {copyError && (
        <p role="alert" className="text-error mt-2 text-xs">
          {m.logs_copy_failed()}
        </p>
      )}
    </article>
  )
}

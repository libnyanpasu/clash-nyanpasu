import ArticleRounded from '~icons/material-symbols/article-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import DataObjectRounded from '~icons/material-symbols/data-object-rounded'
import DeleteSweepRounded from '~icons/material-symbols/delete-sweep-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import VerticalAlignBottomRounded from '~icons/material-symbols/vertical-align-bottom-rounded'
import { useId, useState, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import HighlightText from '@/components/ui/highlight-text'
import { m } from '@/paraglide/messages'
import LogJson from './log-json'
import LogLevelBadge from './log-level-badge'

export const logPanelClass =
  'bg-surface text-on-surface relative mx-3 mb-3 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl'

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
    <div className="shrink-0 px-3 pb-2" data-slot="logs-search">
      <label className="bg-surface-variant/40 text-on-surface-variant focus-within:ring-primary flex h-10 items-center gap-3 rounded-full px-4 focus-within:ring-2">
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

export function LogClearButton({
  disabled,
  onClick,
}: {
  disabled: boolean
  onClick: () => void
}) {
  return (
    <Button
      icon
      disabled={disabled}
      aria-disabled={disabled}
      onClick={onClick}
      aria-label={m.logs_clear_display()}
      title={m.logs_clear_display()}
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
  const [showJson, setShowJson] = useState(false)
  const jsonId = useId()
  const [copied, setCopied] = useState(false)
  const [copyError, setCopyError] = useState(false)
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
          <Button
            icon
            className="focus-visible:ring-primary shrink-0 focus-visible:ring-2"
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
          <Button
            icon
            aria-label={m.logs_view_json()}
            title={m.logs_view_json()}
            aria-expanded={showJson}
            aria-controls={jsonId}
            className="focus-visible:ring-primary shrink-0 focus-visible:ring-2"
            onClick={() => {
              if (!showJson) onInspect()
              setShowJson(!showJson)
            }}
          >
            <DataObjectRounded aria-hidden className="size-4" />
          </Button>
        </div>
      </div>
      <div className="text-on-surface font-mono leading-relaxed [overflow-wrap:anywhere] break-words whitespace-pre-wrap">
        <HighlightText searchText={search}>{message}</HighlightText>
      </div>
      {incomplete && (
        <span
          className="text-on-surface-variant text-xs"
          title={m.logs_raw_record()}
        >
          ⚠
        </span>
      )}
      {showJson && (
        <div id={jsonId} role="region" aria-label={m.logs_view_json()}>
          <LogJson raw={raw} />
        </div>
      )}
      {copyError && (
        <p role="alert" className="text-error mt-2 text-xs">
          {m.logs_copy_failed()}
        </p>
      )}
    </article>
  )
}

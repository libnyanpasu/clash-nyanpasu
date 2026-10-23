import CloseRounded from '~icons/material-symbols/close-rounded'
import KeyboardArrowDownRounded from '~icons/material-symbols/keyboard-arrow-down-rounded'
import KeyboardArrowUpRounded from '~icons/material-symbols/keyboard-arrow-up-rounded'
import type { KeyboardEvent, Ref } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { SearchField } from '@nyanpasu/ui/search-field'
import { cn } from '@nyanpasu/utils'
import { cycleIndex } from './text-find'

/** What a viewer highlights: the text and which of its matches is current. */
export type FindRequest = { query: string; active: number }

/** Ctrl+F, or Cmd+F on macOS. */
export function isFindShortcut(event: KeyboardEvent) {
  return (
    (event.ctrlKey || event.metaKey) &&
    !event.shiftKey &&
    !event.altKey &&
    event.key.toLowerCase() === 'f'
  )
}

/**
 * Scrolls `container` just enough to show `target`, centering it when it is
 * out of view. Unlike `scrollIntoView`, it never moves an ancestor.
 */
export function revealWithin(container: HTMLElement, target: HTMLElement) {
  const box = container.getBoundingClientRect()
  const rect = target.getBoundingClientRect()
  if (rect.top >= box.top && rect.bottom <= box.bottom) return
  container.scrollTop +=
    rect.top - box.top - (container.clientHeight - rect.height) / 2
}

/**
 * Find in text with next and previous. The text of every label is a prop, so
 * the bar is not tied to one page.
 */
export function FindBar({
  query,
  onQueryChange,
  total,
  active,
  onActiveChange,
  onClose,
  status,
  label,
  clearLabel,
  previousLabel,
  nextLabel,
  closeLabel,
  className,
  inputRef,
}: {
  query: string
  onQueryChange: (query: string) => void
  /** How many matches there are. */
  total: number
  /** The current match, from 0. */
  active: number
  onActiveChange: (active: number) => void
  onClose: () => void
  /** Reads like "2 of 5". */
  status: string
  label: string
  clearLabel: string
  previousLabel: string
  nextLabel: string
  closeLabel: string
  className?: string
  inputRef?: Ref<HTMLInputElement>
}) {
  const step = (delta: 1 | -1) =>
    onActiveChange(cycleIndex(active, total, delta))

  return (
    <div
      role="search"
      aria-label={label}
      data-slot="find-bar"
      className={cn('flex min-w-0 items-center gap-1', className)}
    >
      <SearchField
        ref={inputRef}
        className="min-w-0 flex-1"
        value={query}
        onValueChange={onQueryChange}
        clearLabel={clearLabel}
        placeholder={label}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault()
            step(event.shiftKey ? -1 : 1)
          } else if (event.key === 'Escape') {
            event.stopPropagation()
            onClose()
          }
        }}
      />

      <span
        role="status"
        data-slot="find-bar-status"
        className="text-on-surface-variant min-w-16 shrink-0 text-center text-xs tabular-nums"
      >
        {query ? status : ''}
      </span>

      <Button
        type="button"
        icon
        className="size-9 shrink-0"
        disabled={total === 0}
        aria-label={previousLabel}
        data-slot="find-bar-previous"
        onClick={() => step(-1)}
      >
        <KeyboardArrowUpRounded className="size-5" aria-hidden />
      </Button>

      <Button
        type="button"
        icon
        className="size-9 shrink-0"
        disabled={total === 0}
        aria-label={nextLabel}
        data-slot="find-bar-next"
        onClick={() => step(1)}
      >
        <KeyboardArrowDownRounded className="size-5" aria-hidden />
      </Button>

      <Button
        type="button"
        icon
        className="size-9 shrink-0"
        aria-label={closeLabel}
        data-slot="find-bar-close"
        onClick={onClose}
      >
        <CloseRounded className="size-5" aria-hidden />
      </Button>
    </div>
  )
}

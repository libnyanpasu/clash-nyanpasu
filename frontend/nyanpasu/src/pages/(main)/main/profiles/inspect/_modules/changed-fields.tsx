import { useState } from 'react'
import HighlightText from '@nyanpasu/ui/highlight-text'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

const COLLAPSED_COUNT = 8

const fieldClassName = cn(
  'bg-surface-variant text-on-surface-variant dark:bg-surface-variant',
  'inline-flex h-[26px] items-center rounded-lg px-[9px] font-mono text-[11.5px]',
)

export default function ChangedFields({
  fields,
  query = '',
  onSelect,
  canSelect = () => true,
}: {
  fields: string[] | null
  /** Marks the part of each field name that matches. */
  query?: string
  /** Makes each field a button, such as one that scrolls a viewer to it. */
  onSelect?: (field: string) => void
  /** A field that cannot be located is shown, but cannot be chosen. */
  canSelect?: (field: string) => boolean
}) {
  const [expanded, setExpanded] = useState(false)

  // The summary and the viewer already say that nothing changed.
  if (fields === null || fields.length === 0) return null

  const collapsible = fields.length > COLLAPSED_COUNT
  const visible =
    collapsible && !expanded ? fields.slice(0, COLLAPSED_COUNT) : fields

  return (
    <div
      data-slot="inspect-changed-fields"
      className="flex max-h-32 flex-wrap items-center gap-2.5 overflow-y-auto"
    >
      {visible.map((field) =>
        onSelect ? (
          <button
            key={field}
            type="button"
            data-slot="inspect-changed-field"
            disabled={!canSelect(field)}
            className={cn(
              fieldClassName,
              'focus-visible:outline-primary cursor-pointer outline-offset-1 focus-visible:outline-2',
              'enabled:hover:brightness-95 dark:enabled:hover:brightness-125',
              'disabled:cursor-default disabled:opacity-60',
            )}
            onClick={() => onSelect(field)}
          >
            <HighlightText searchText={query}>{field}</HighlightText>
          </button>
        ) : (
          <code key={field} className={fieldClassName}>
            <HighlightText searchText={query}>{field}</HighlightText>
          </code>
        ),
      )}

      {collapsible && (
        <button
          type="button"
          className={cn(
            fieldClassName,
            'text-primary cursor-pointer hover:brightness-95 dark:hover:brightness-125',
          )}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded
            ? m.inspect_fields_collapse()
            : `+${fields.length - COLLAPSED_COUNT}`}
        </button>
      )}
    </div>
  )
}

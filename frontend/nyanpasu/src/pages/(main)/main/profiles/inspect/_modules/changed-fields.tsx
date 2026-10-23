import { useState } from 'react'
import HighlightText from '@nyanpasu/ui/highlight-text'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

const COLLAPSED_COUNT = 8

const fieldClassName = cn(
  'bg-surface-variant/40 dark:bg-surface-variant/20',
  'rounded-md px-1.5 py-0.5 font-mono text-xs',
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

  if (fields === null || fields.length === 0) {
    return (
      <p className="text-on-surface-variant text-sm">
        {m.inspect_fields()}:{' '}
        {fields === null ? m.inspect_no_baseline() : m.inspect_unchanged()}
      </p>
    )
  }

  const collapsible = fields.length > COLLAPSED_COUNT
  const visible =
    collapsible && !expanded ? fields.slice(0, COLLAPSED_COUNT) : fields

  return (
    <div className="flex flex-wrap items-center gap-1.5 text-sm">
      <span className="text-on-surface-variant mr-1">{m.inspect_fields()}</span>

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
              'enabled:hover:bg-surface-variant/70 disabled:cursor-default disabled:opacity-60',
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
            'text-primary hover:bg-surface-variant/60 cursor-pointer',
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

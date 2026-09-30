import { useState } from 'react'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

const COLLAPSED_COUNT = 8

const fieldClassName = cn(
  'bg-surface-variant/40 dark:bg-surface-variant/20',
  'rounded-md px-1.5 py-0.5 font-mono text-xs',
)

export default function ChangedFields({ fields }: { fields: string[] | null }) {
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

      {visible.map((field) => (
        <code key={field} className={fieldClassName}>
          {field}
        </code>
      ))}

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

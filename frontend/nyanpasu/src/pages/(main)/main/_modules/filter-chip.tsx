import CloseRounded from '~icons/material-symbols/close-rounded'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

export default function FilterChip({
  label,
  title,
  onRemove,
  className,
}: {
  /** "Dimension: value" */
  label: string
  title?: string
  onRemove: () => void
  className?: string
}) {
  return (
    <span
      className={cn(
        'border-outline-variant text-on-surface flex h-8 shrink-0 items-center gap-1 rounded-lg border pr-1 pl-3 text-sm',
        className,
      )}
      data-slot="traffic-filter-chip"
      title={title}
    >
      <span className="max-w-56 truncate">{label}</span>

      <button
        type="button"
        className="hover:bg-on-surface/8 grid size-6 shrink-0 cursor-pointer place-content-center rounded-md"
        aria-label={m.traffic_filter_remove({ filter: label })}
        onClick={onRemove}
      >
        <CloseRounded className="size-4" />
      </button>
    </span>
  )
}

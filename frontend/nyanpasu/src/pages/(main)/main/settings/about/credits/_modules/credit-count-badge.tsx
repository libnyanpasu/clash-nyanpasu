import { cn } from '@nyanpasu/utils'

/** A count beside a label; inside a pressed filter chip it takes its colors. */
export default function CreditCountBadge({ count }: { count: number }) {
  return (
    <span
      className={cn(
        'inline-block rounded-full px-1.5 text-[11px] leading-4 font-medium tabular-nums',
        'bg-on-surface/8 text-on-surface-variant',
        'group-data-[state=on]:bg-on-secondary-container/12',
        'group-data-[state=on]:text-on-secondary-container',
      )}
      data-slot="credits-count-badge"
    >
      {count.toLocaleString()}
    </span>
  )
}

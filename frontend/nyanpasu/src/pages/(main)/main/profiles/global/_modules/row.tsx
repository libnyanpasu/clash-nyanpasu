import { cva } from 'class-variance-authority'
import { cardVariants } from '@nyanpasu/ui/card'

/** A card row of the pipeline and the library; a link row is interactive. */
export const rowVariants = cva(
  [cardVariants(), 'flex min-w-0 items-center gap-3 py-2.5 pr-2.5 pl-3'],
  {
    variants: {
      interactive: {
        true: [
          'transition-colors motion-reduce:transition-none',
          'hover:bg-surface-variant/60',
          'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
        ],
        false: '',
      },
    },
    defaultVariants: { interactive: false },
  },
)

/** The name of a profile with its type chip, the body of every row. */
export function RowTitle({ name, type }: { name: string; type?: string }) {
  return (
    <div className="min-w-0 flex-1">
      <p className="text-sm font-medium [overflow-wrap:anywhere]">{name}</p>

      {type && (
        <span
          data-slot="profile-global-row-type"
          className="bg-secondary-container/60 text-on-secondary-container mt-1 inline-block rounded-md px-1.5 py-0.5 text-xs"
        >
          {type}
        </span>
      )}
    </div>
  )
}

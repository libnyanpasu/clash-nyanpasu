import { cva } from 'class-variance-authority'
import { cn } from '@nyanpasu/utils'
import type { LaneRow } from './inspect-lanes'

/** The width of one lane in the gutter, in pixels. */
export const LANE_WIDTH = 14

const lineVariants = cva('absolute', {
  variants: {
    lane: { main: 'bg-outline', branch: 'bg-primary' },
  },
})

const dotVariants = cva('absolute top-1/2 -translate-x-1/2 -translate-y-1/2', {
  variants: {
    lane: { main: '', branch: '' },
    shape: {
      start: 'bg-pane size-2.5 rounded-full border-2',
      merge: 'size-2.5 rotate-45 rounded-[2px]',
      fork: 'size-2.5 rounded-[3px]',
      step: 'size-2 rounded-full',
    },
    selected: { true: 'ring-primary/20 ring-4', false: '' },
  },
  compoundVariants: [
    {
      shape: ['merge', 'fork', 'step'],
      lane: 'main',
      selected: false,
      className: 'bg-outline',
    },
    {
      shape: ['merge', 'fork', 'step'],
      lane: 'branch',
      selected: false,
      className: 'bg-primary',
    },
    {
      shape: ['merge', 'fork', 'step'],
      selected: true,
      className: 'bg-primary',
    },
    {
      shape: 'start',
      lane: 'main',
      selected: false,
      className: 'border-outline',
    },
    { shape: 'start', lane: 'branch', className: 'border-primary' },
    { shape: 'start', selected: true, className: 'border-primary' },
  ],
})

const laneTone = (lane: number) => (lane === 0 ? 'main' : 'branch')
const laneX = (lane: number) => lane * LANE_WIDTH + LANE_WIDTH / 2

/**
 * The branch gutter of a row or of the space between rows. It sits to the left
 * of the positioned parent, outside the row, so the row keeps its own shape.
 * Without a `row` it only carries the lanes that run through.
 */
export default function LaneGutter({
  laneCount,
  through,
  row,
  selected = false,
  className,
}: {
  laneCount: number
  /** Lanes that run straight through. */
  through: readonly number[]
  row?: LaneRow
  selected?: boolean
  className?: string
}) {
  const vertical = (
    lane: number,
    from: 'top' | 'middle',
    to: 'bottom' | 'middle',
    key: string,
  ) => (
    <span
      key={key}
      className={cn(
        lineVariants({ lane: laneTone(lane) }),
        'w-0.5 -translate-x-1/2',
        from === 'top' ? 'top-0' : 'top-1/2',
        to === 'bottom' ? 'bottom-0' : 'bottom-1/2',
      )}
      style={{ left: laneX(lane) }}
    />
  )
  const horizontal = (lane: number, key: string) => {
    const own = row!.lane
    return (
      <span
        key={key}
        className={cn(
          lineVariants({ lane: laneTone(lane) }),
          'top-1/2 h-0.5 -translate-y-1/2',
        )}
        style={{
          left: laneX(Math.min(lane, own)),
          width: Math.abs(laneX(lane) - laneX(own)) + 2,
        }}
      />
    )
  }

  return (
    <span
      aria-hidden
      data-slot="inspect-lane-gutter"
      className={cn(
        'pointer-events-none absolute inset-y-0 right-[calc(100%+4px)]',
        className,
      )}
      style={{ width: laneCount * LANE_WIDTH }}
    >
      {through.map((lane) =>
        vertical(lane, 'top', 'bottom', `through-${lane}`),
      )}

      {row && (
        <>
          {row.incoming && vertical(row.lane, 'top', 'middle', 'in')}
          {row.outgoing && vertical(row.lane, 'middle', 'bottom', 'out')}

          {row.merges.map((lane) => [
            vertical(lane, 'top', 'middle', `merge-${lane}`),
            horizontal(lane, `merge-join-${lane}`),
          ])}

          {row.forks.map((lane) => [
            vertical(lane, 'middle', 'bottom', `fork-${lane}`),
            horizontal(lane, `fork-join-${lane}`),
          ])}

          <span
            className={dotVariants({
              lane: laneTone(row.lane),
              shape: row.shape,
              selected,
            })}
            style={{ left: laneX(row.lane) }}
          />
        </>
      )}
    </span>
  )
}

import { m } from '@/paraglide/messages'

// The widths of the placeholder lines, as the lines of a YAML document vary.
const LINE_WIDTHS = ['30%', '55%', '45%', '62%', '38%', '50%', '26%', '58%']
// Enough lines for the tallest panel; the ones below its bottom are clipped.
const LINE_COUNT = 120

/** Holds the place of a step's document while it loads or mounts. */
export default function StepPanelSkeleton() {
  return (
    <div
      role="status"
      aria-label={m.inspect_loading()}
      data-slot="inspect-step-panel-skeleton"
      className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden px-5 py-3 motion-safe:animate-pulse"
    >
      {Array.from({ length: LINE_COUNT }, (_, index) => (
        <div key={index} className="flex h-3 shrink-0 items-center gap-4">
          <div className="bg-surface-variant h-full w-6 rounded-full" />

          <div
            className="bg-surface-variant/60 h-full rounded-full"
            style={{ width: LINE_WIDTHS[index % LINE_WIDTHS.length] }}
          />
        </div>
      ))}
    </div>
  )
}

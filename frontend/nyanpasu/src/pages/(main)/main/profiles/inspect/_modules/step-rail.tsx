import { useEffect, useMemo, useRef } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import type { ProfileLookup } from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import {
  StageLabel,
  StepGroup,
  StepRowText,
  stepRowVariants,
} from '../../_modules/step-list'
import { revealWithin } from './find-bar'
import { stageLabel, stepHeadline, stepMeta, stepSub } from './inspect-labels'
import { layoutLanes } from './inspect-lanes'
import { STAGES, type Step } from './inspect-steps'
import LaneGutter, { LANE_WIDTH } from './lane-gutter'
import OnlyChangedSwitch from './only-changed-switch'
import StatusIcon from './status-icon'

/**
 * The steps of the build, grouped by stage. A step is chosen by its key, which
 * stays the same when the snapshot is rebuilt.
 */
export default function StepRail({
  steps,
  allSteps,
  selectedKey,
  onSelect,
  profiles,
  targetCore,
  onlyChanged,
  onOnlyChangedChange,
  matchedKeys,
  className,
}: {
  /** The steps the list shows, after the "only changed" filter. */
  steps: Step[]
  /** Every step, whatever the filter, for the position inside a chain. */
  allSteps: Step[]
  selectedKey?: string
  onSelect: (key: string) => void
  profiles: ProfileLookup
  targetCore: string
  onlyChanged: boolean
  onOnlyChangedChange: (onlyChanged: boolean) => void
  /** The steps that match the field query; absent when there is no query. */
  matchedKeys?: ReadonlySet<string>
  className?: string
}) {
  const listRef = useRef<HTMLDivElement>(null)

  const layout = useMemo(() => layoutLanes(allSteps, steps), [allSteps, steps])
  const rows = new Map(layout.rows.map((row) => [row.key, row]))
  const stages = STAGES.filter((stage) =>
    steps.some((step) => step.stage === stage),
  )

  // Keep the chosen step in sight, such as one a link or a search picked.
  useEffect(() => {
    const list = listRef.current
    const row = list?.querySelector<HTMLElement>('[aria-current=step]')
    const viewport = list?.closest<HTMLElement>(
      '[data-slot=scroll-area-viewport]',
    )
    if (row && viewport) revealWithin(viewport, row)
  }, [selectedKey, steps])

  return (
    <aside
      data-slot="inspect-step-rail"
      aria-label={m.inspect_steps()}
      className={cn(
        'bg-pane flex min-h-0 min-w-0 flex-col',
        'shadow-[inset_-1px_0_0_var(--color-outline-variant)]',
        className,
      )}
    >
      <ScrollArea
        data-slot="inspect-step-rail-scroll-area"
        className="min-h-0 flex-1"
      >
        <div
          ref={listRef}
          className="flex flex-col gap-[18px] pt-3.5 pr-3 pb-3"
          // The gutter hangs to the left of the rows, 4px from them.
          style={{ paddingLeft: 12 + layout.laneCount * LANE_WIDTH + 4 }}
        >
          <p
            data-slot="inspect-rail-hint"
            className="bg-surface text-on-surface-variant rounded-2xl px-3.5 py-3 text-[12.5px] leading-[18px]"
          >
            {m.inspect_rail_hint()}
          </p>

          {stages.map((stage, stageIndex) => {
            const group = steps.filter((step) => step.stage === stage)
            const first = rows.get(group[0].key)!
            const last = rows.get(group.at(-1)!.key)!
            // The lanes that are open between the stage before and this one.
            const before = layout.rows[layout.rows.indexOf(first) - 1]?.after

            return (
              <div key={stage} data-stage={stage} className="relative">
                <div className="relative">
                  <LaneGutter
                    laneCount={layout.laneCount}
                    through={before ?? []}
                  />

                  <StageLabel>{stageLabel(stage)}</StageLabel>
                </div>

                <StepGroup>
                  {group.map((step) => {
                    const selected = step.key === selectedKey
                    const matched = matchedKeys?.has(step.key) ?? false
                    const meta = stepMeta(step, matched)
                    const row = rows.get(step.key)!

                    return (
                      <button
                        key={step.key}
                        type="button"
                        aria-current={selected ? 'step' : undefined}
                        data-selected={selected ? '' : undefined}
                        data-key={step.key}
                        data-status={step.status}
                        data-dimmed={matchedKeys && !matched ? '' : undefined}
                        className={cn(
                          stepRowVariants({ interactive: true }),
                          step.depth === 1 && 'ms-6 w-[calc(100%-1.5rem)]',
                          'data-dimmed:opacity-40',
                        )}
                        onClick={() => onSelect(step.key)}
                      >
                        <LaneGutter
                          laneCount={layout.laneCount}
                          through={row.through}
                          row={row}
                          selected={selected}
                          className={cn(
                            '-bottom-0.5',
                            step.depth === 1 && '-translate-x-6',
                          )}
                        />

                        <StatusIcon status={step.status} />

                        <StepRowText
                          title={stepHeadline(step, profiles)}
                          sub={stepSub(step, allSteps, profiles, targetCore)}
                        />

                        {meta && (
                          <span className="text-on-surface-variant group-data-selected/row:text-on-secondary-container shrink-0 text-xs whitespace-nowrap group-data-selected/row:opacity-80">
                            {meta}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </StepGroup>

                {stageIndex < stages.length - 1 && (
                  <LaneGutter
                    laneCount={layout.laneCount}
                    through={last.after}
                    className="inset-y-auto top-full h-[18px]"
                  />
                )}
              </div>
            )
          })}
        </div>
      </ScrollArea>

      <OnlyChangedSwitch
        className="mx-3 mt-1 mb-4"
        checked={onlyChanged}
        onCheckedChange={onOnlyChangedChange}
      />
    </aside>
  )
}

import CircleRounded from '~icons/material-symbols/circle-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import InputRounded from '~icons/material-symbols/input-rounded'
import OutputRounded from '~icons/material-symbols/output-rounded'
import RadioButtonUncheckedRounded from '~icons/material-symbols/radio-button-unchecked-rounded'
import StarRounded from '~icons/material-symbols/star-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { useEffect, useRef, type ComponentType, type SVGProps } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { Switch } from '@nyanpasu/ui/switch'
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
import {
  stageLabel,
  statusLabel,
  stepHeadline,
  stepMeta,
  stepSub,
} from './inspect-labels'
import { STAGES, type Step, type StepStatus } from './inspect-steps'

const STATUS_ICON: Record<
  StepStatus,
  { icon: ComponentType<SVGProps<SVGSVGElement>>; className: string }
> = {
  start: { icon: InputRounded, className: 'text-ok' },
  changed: { icon: CircleRounded, className: 'text-ok' },
  unchanged: {
    icon: RadioButtonUncheckedRounded,
    className: 'text-on-surface-variant opacity-60',
  },
  warn: { icon: WarningRounded, className: 'text-warn' },
  error: { icon: ErrorRounded, className: 'text-error' },
  final: { icon: StarRounded, className: 'text-ok' },
  core: { icon: OutputRounded, className: 'text-ok' },
}

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
          className="flex flex-col gap-[18px] px-3 pt-3.5 pb-3"
        >
          <p
            data-slot="inspect-rail-hint"
            className="bg-surface text-on-surface-variant rounded-2xl px-3.5 py-3 text-[12.5px] leading-[18px]"
          >
            {m.inspect_rail_hint()}
          </p>

          {STAGES.map((stage) => {
            const group = steps.filter((step) => step.stage === stage)
            if (group.length === 0) return null

            return (
              <div key={stage} data-stage={stage}>
                <StageLabel>{stageLabel(stage)}</StageLabel>

                <StepGroup>
                  {group.map((step) => {
                    const selected = step.key === selectedKey
                    const matched = matchedKeys?.has(step.key) ?? false
                    const { icon: Icon, className: iconClassName } =
                      STATUS_ICON[step.status]
                    const meta = stepMeta(step, matched)

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
                        <Icon
                          role="img"
                          aria-label={statusLabel(step.status)}
                          className={cn('size-5 shrink-0', iconClassName)}
                        />

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
              </div>
            )
          })}
        </div>
      </ScrollArea>

      <label
        data-slot="inspect-only-changed"
        className="text-on-surface-variant mx-3 mt-1 mb-4 flex shrink-0 cursor-pointer items-center justify-between gap-3 px-2 text-[13px]"
      >
        {m.inspect_only_changed()}

        <Switch checked={onlyChanged} onCheckedChange={onOnlyChangedChange} />
      </label>
    </aside>
  )
}

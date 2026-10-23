import EditRounded from '~icons/material-symbols/edit-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import FlagRounded from '~icons/material-symbols/flag-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import RemoveRounded from '~icons/material-symbols/remove-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { useEffect, useRef, type ComponentType, type SVGProps } from 'react'
import {
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import type { ProfileLookup } from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import type { RuntimeInspectionNode } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import ChainGraph from './chain-graph'
import { revealWithin } from './find-bar'
import { stageLabel, statusLabel, stepHeadline } from './inspect-labels'
import { STAGES, type Step, type StepStatus } from './inspect-steps'

export type RailView = 'list' | 'chain'

const STATUS_STYLE: Record<
  StepStatus,
  {
    color: NavigationListItemColor
    icon: ComponentType<SVGProps<SVGSVGElement>>
  }
> = {
  start: { color: 'green', icon: PlayArrowRounded },
  changed: { color: 'blue', icon: EditRounded },
  unchanged: { color: 'neutral', icon: RemoveRounded },
  warn: { color: 'yellow', icon: WarningRounded },
  error: { color: 'red', icon: ErrorRounded },
  final: { color: 'purple', icon: FlagRounded },
  core: { color: 'cyan', icon: MemoryOutlineRounded },
}

function stepSupporting(step: Step, matched: boolean) {
  if (matched) return m.inspect_step_matches()
  if (step.status === 'start') return m.inspect_step_start()
  return (step.changedCount ?? 0) > 0
    ? m.inspect_step_changed({ count: step.changedCount! })
    : m.inspect_step_unchanged()
}

/**
 * The steps of the build, grouped by stage. A step is chosen by its key, which
 * stays the same when the snapshot is rebuilt.
 */
export default function StepRail({
  steps,
  allSteps,
  nodes,
  selectedKey,
  onSelect,
  profiles,
  onOpenProfile,
  view,
  onViewChange,
  onlyChanged,
  onOnlyChangedChange,
  matchedKeys,
  className,
}: {
  /** The steps the list shows, after the "only changed" filter. */
  steps: Step[]
  /** Every step, which the chain keeps so that its edges stay connected. */
  allSteps: Step[]
  nodes: RuntimeInspectionNode[]
  selectedKey?: string
  onSelect: (key: string) => void
  profiles: ProfileLookup
  onOpenProfile: (id: string) => void
  view: RailView
  onViewChange: (view: RailView) => void
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
  }, [selectedKey, view, steps])

  const selectedId = allSteps.find((step) => step.key === selectedKey)?.node.id

  return (
    <aside
      data-slot="inspect-step-rail"
      className={cn('flex min-h-0 min-w-0 flex-col', className)}
    >
      <header className="flex shrink-0 items-start justify-between gap-3 px-4 pt-2 pb-1">
        <div className="min-w-0">
          <h2 className="text-on-primary-container text-sm font-medium">
            {m.inspect_steps()}
          </h2>

          <p className="text-on-surface-variant text-xs">
            {m.inspect_steps_purpose()}
          </p>
        </div>

        <SegmentedButton
          className="w-40 shrink-0"
          size="sm"
          aria-label={m.inspect_view_label()}
          value={view}
          onValueChange={(value) => {
            if (value) onViewChange(value as RailView)
          }}
        >
          <SegmentedButtonItem value="list">
            {m.inspect_view_list()}
          </SegmentedButtonItem>
          <SegmentedButtonItem value="chain">
            {m.inspect_view_chain()}
          </SegmentedButtonItem>
        </SegmentedButton>
      </header>

      {view === 'list' ? (
        <ScrollArea
          data-slot="inspect-step-rail-scroll-area"
          className="min-h-0 flex-1"
        >
          <div ref={listRef}>
            <NavigationList aria-label={m.inspect_steps()}>
              {STAGES.map((stage) => {
                const group = steps.filter((step) => step.stage === stage)
                if (group.length === 0) return null

                return (
                  <NavigationListGroup
                    key={stage}
                    label={stageLabel(stage)}
                    data-stage={stage}
                  >
                    {group.map((step) => {
                      const selected = step.key === selectedKey
                      const matched = matchedKeys?.has(step.key) ?? false
                      const { color, icon: Icon } = STATUS_STYLE[step.status]

                      return (
                        <NavigationListItem
                          key={step.key}
                          active={selected}
                          aria-current={selected ? 'step' : undefined}
                          data-key={step.key}
                          data-status={step.status}
                          data-dimmed={matchedKeys && !matched ? '' : undefined}
                          className={cn(
                            'min-h-12 py-1.5',
                            step.depth === 1 && 'ms-6 w-[calc(100%-1.5rem)]',
                            'data-dimmed:opacity-40',
                          )}
                          onClick={() => onSelect(step.key)}
                        >
                          <NavigationListItemIcon
                            color={color}
                            className="size-7"
                            role="img"
                            aria-hidden={false}
                            aria-label={statusLabel(step.status)}
                          >
                            <Icon />
                          </NavigationListItemIcon>

                          <NavigationListItemText
                            headline={stepHeadline(step, profiles)}
                            supporting={stepSupporting(step, matched)}
                          />
                        </NavigationListItem>
                      )
                    })}
                  </NavigationListGroup>
                )
              })}
            </NavigationList>
          </div>
        </ScrollArea>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col px-3 pt-2">
          <ChainGraph
            nodes={nodes}
            profiles={profiles}
            onOpenProfile={onOpenProfile}
            selectedId={selectedId}
            onSelect={(id) => {
              const step = allSteps.find((step) => step.node.id === id)
              if (step) onSelect(step.key)
            }}
          />
        </div>
      )}

      {view === 'list' && (
        <label className="flex shrink-0 items-center justify-between gap-3 px-4 py-3 text-sm">
          {m.inspect_only_changed()}
          <Switch checked={onlyChanged} onCheckedChange={onOnlyChangedChange} />
        </label>
      )}
    </aside>
  )
}

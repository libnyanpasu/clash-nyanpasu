import CloseRounded from '~icons/material-symbols/close-rounded'
import { useMemo, useState, type ComponentProps, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerHandle,
  DrawerTitle,
} from '@nyanpasu/ui/drawer'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { ErrorMessage } from '@/components/error-message'
import type { ProfileLookup } from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import type { RuntimeInspection } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import type { UseQueryResult } from '@tanstack/react-query'
import {
  buildSteps,
  filterSteps,
  matchField,
  resolveStep,
} from './inspect-steps'
import InspectToolbar, { type StepLayout } from './inspect-toolbar'
import type { InspectView } from './inspect-view'
import StepDetail, { defaultTab, type InspectTab } from './step-detail'
import StepGraph from './step-graph'
import StepRail from './step-rail'

export type InspectionSource = {
  summary: Pick<
    UseQueryResult<RuntimeInspection | null>,
    'data' | 'isPending' | 'isError' | 'error'
  >
  loadNode: (
    snapshotId: string,
    nodeId: number,
  ) => ComponentProps<typeof StepDetail>['contentQuery']
}

export default function InspectViewer({
  source,
  selectedStep: stepParam,
  onStepChange: selectStep,
  inspectView,
  onViewChange: updateView,
  profiles = new Map(),
  onOpenProfile,
  globalLink,
  snapshotControls,
  status,
  onRefresh,
}: {
  source: InspectionSource
  selectedStep?: string
  onStepChange: (step: string) => void
  inspectView: InspectView
  onViewChange: (update: Partial<InspectView>) => void
  profiles?: ProfileLookup
  onOpenProfile?: (id: string) => void
  globalLink?: ReactNode
  snapshotControls?: ReactNode
  status?: ReactNode
  onRefresh?: () => void
}) {
  const inspection = source.summary
  const snapshot = inspection.data
  const [layout, setLayout] = useState<StepLayout>('list')
  const { onlyChanged } = inspectView
  const setOnlyChanged = (next: boolean) => updateView({ onlyChanged: next })
  const [fieldQuery, setFieldQuery] = useState('')
  const [stepsOpen, setStepsOpen] = useState(false)
  const [tabOverride, setTabOverride] = useState<{
    key: string
    tab: InspectTab
  }>()

  const steps = useMemo(
    () => (snapshot ? buildSteps(snapshot.nodes, snapshot.final_id) : []),
    [snapshot],
  )
  // Keep the step key across snapshots; a missing key falls back to the final step.
  const selected = resolveStep(steps, stepParam)
  const visibleSteps = filterSteps(steps, {
    onlyChanged,
    selectedKey: selected?.key,
  })
  const matchedKeys = useMemo(
    () =>
      fieldQuery.trim()
        ? new Set(
            steps
              .filter((step) => matchField(step, fieldQuery))
              .map((step) => step.key),
          )
        : undefined,
    [steps, fieldQuery],
  )

  // Enter in the search steps through the steps that changed a matching field.
  const cycleMatch = (delta: 1 | -1) => {
    if (!matchedKeys || !selected) return
    const matches = steps.filter((step) => matchedKeys.has(step.key))
    if (matches.length === 0) return
    const position = steps.indexOf(selected)
    const next =
      delta === 1
        ? (matches.find((step) => steps.indexOf(step) > position) ?? matches[0])
        : (matches.findLast((step) => steps.indexOf(step) < position) ??
          matches.at(-1))
    if (next) selectStep(next.key)
  }

  const tab =
    selected && tabOverride?.key === selected.key
      ? tabOverride.tab
      : selected && (inspectView.documentTab ?? defaultTab(selected))

  const graph = (className?: string) =>
    snapshot && (
      <StepGraph
        className={className}
        steps={visibleSteps}
        allSteps={steps}
        selectedKey={selected?.key}
        onSelect={selectStep}
        profiles={profiles}
        onlyChanged={onlyChanged}
        onOnlyChangedChange={setOnlyChanged}
        matchedKeys={matchedKeys}
      />
    )

  const rail = (className?: string, onSelected?: () => void) =>
    snapshot && (
      <StepRail
        className={className}
        steps={visibleSteps}
        allSteps={steps}
        selectedKey={selected?.key}
        onSelect={(key) => {
          selectStep(key)
          onSelected?.()
        }}
        profiles={profiles}
        targetCore={snapshot.target_core}
        onlyChanged={onlyChanged}
        onOnlyChangedChange={setOnlyChanged}
        matchedKeys={matchedKeys}
      />
    )

  return (
    <section
      data-slot="inspect-page"
      className="@container flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
    >
      <InspectToolbar
        snapshot={snapshot}
        snapshotControls={snapshotControls}
        status={status}
        layout={layout}
        onLayoutChange={setLayout}
        fieldQuery={fieldQuery}
        onFieldQueryChange={setFieldQuery}
        onCycleMatch={cycleMatch}
        onRefresh={onRefresh}
        onShowSteps={() => setStepsOpen(true)}
      />

      {inspection.isPending && (
        <p
          role="status"
          className="text-on-surface-variant flex flex-1 items-center justify-center text-sm"
        >
          {m.inspect_loading()}
        </p>
      )}

      {inspection.isError && (
        <p
          role="alert"
          className="flex flex-1 flex-wrap content-center items-center justify-center gap-1 px-4 text-sm"
        >
          {m.inspect_error()} <ErrorMessage error={inspection.error} />
        </p>
      )}

      {!inspection.isError && snapshot === null && (
        <EmptyState
          tone="normal"
          className="flex-1"
          message={m.inspect_empty()}
        />
      )}

      {!inspection.isError && snapshot && selected && tab && (
        <div
          className={cn(
            'grid min-h-0 flex-1',
            'grid-rows-[minmax(0,1fr)] @3xl:grid-cols-[20rem_minmax(0,1fr)]',
            // Stacked, the graph gets the smaller share.
            layout === 'graph' &&
              'grid-rows-[minmax(0,2fr)_minmax(0,3fr)] @3xl:grid-rows-[minmax(0,1fr)]',
          )}
        >
          {layout === 'graph' ? graph() : rail('hidden @3xl:flex')}

          <StepDetail
            key={selected.key}
            snapshotId={snapshot.snapshot_id}
            contentQuery={source.loadNode(
              snapshot.snapshot_id,
              selected.node.id,
            )}
            step={selected}
            steps={steps}
            profiles={profiles}
            targetCore={snapshot.target_core}
            onOpenProfile={onOpenProfile}
            globalLink={globalLink}
            tab={tab}
            onTabChange={(next) => {
              setTabOverride({ key: selected.key, tab: next })
              if (next !== 'logs') updateView({ documentTab: next })
            }}
            fieldQuery={fieldQuery}
          />
        </div>
      )}

      <Drawer open={stepsOpen} onOpenChange={setStepsOpen}>
        <DrawerContent
          aria-describedby={undefined}
          className="h-[85dvh] max-h-[85dvh] min-h-0 w-[calc(100%-1rem)] max-w-[960px]"
        >
          <DrawerHandle />

          <div className="flex shrink-0 items-center justify-between gap-4 px-4 pt-2">
            <DrawerTitle className="text-lg font-semibold">
              {m.inspect_steps()}
            </DrawerTitle>

            <DrawerClose asChild>
              <Button
                variant="raised"
                className="size-8"
                icon
                aria-label={m.inspect_hide_steps()}
              >
                <CloseRounded className="size-4" />
              </Button>
            </DrawerClose>
          </div>

          {rail('min-h-0 flex-1', () => setStepsOpen(false))}
        </DrawerContent>
      </Drawer>
    </section>
  )
}

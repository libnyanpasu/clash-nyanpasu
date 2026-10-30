import CloseRounded from '~icons/material-symbols/close-rounded'
import { useEffect, useMemo, useState } from 'react'
import { z } from 'zod'
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
import { useOpenProfile, useProfileLookup } from '@/components/profile-label'
import { keepReturn } from '@/components/router/cross-navigation'
import { m } from '@/paraglide/messages'
import { events, queries } from '@/services/rpc'
import { unwrapQueryOptions, useKvStorage } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import {
  useQuery,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import {
  buildSteps,
  filterSteps,
  matchField,
  resolveStep,
} from './_modules/inspect-steps'
import InspectToolbar, {
  type SnapshotView,
  type StepLayout,
} from './_modules/inspect-toolbar'
import {
  DEFAULT_INSPECT_VIEW,
  INSPECT_VIEW_KV_KEY,
  toInspectView,
  type InspectView,
} from './_modules/inspect-view'
import StepDetail, { defaultTab, type InspectTab } from './_modules/step-detail'
import StepGraph from './_modules/step-graph'
import StepRail from './_modules/step-rail'

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
  validateSearch: z.object({ step: z.string().optional() }),
  staticData: { layout: 'fill' },
})

function invalidateSnapshots(queryClient: QueryClient) {
  for (const query of [
    queries.inspectRuntime(),
    queries.inspectAppliedRuntime(),
  ]) {
    queryClient.invalidateQueries({ queryKey: query.queryKey })
  }
}

function RouteComponent() {
  const queryClient = useQueryClient()
  const navigate = useNavigate({ from: Route.fullPath })
  const { step: stepParam } = Route.useSearch()
  const profiles = useProfileLookup()
  const openProfile = useOpenProfile(profiles)

  const [view, setView] = useState<SnapshotView>('latest')
  const [layout, setLayout] = useState<StepLayout>('list')
  const [inspectView, setInspectView] = useKvStorage(
    INSPECT_VIEW_KV_KEY,
    DEFAULT_INSPECT_VIEW,
    { migrate: toInspectView },
  )
  // A view that fails to save still applies; it is only not remembered.
  const updateView = (update: Partial<InspectView>) => {
    setInspectView((previous) => ({ ...previous, ...update }))
  }
  const { onlyChanged } = inspectView
  const setOnlyChanged = (next: boolean) => updateView({ onlyChanged: next })
  const [fieldQuery, setFieldQuery] = useState('')
  const [stepsOpen, setStepsOpen] = useState(false)
  const [tabOverride, setTabOverride] = useState<{
    key: string
    tab: InspectTab
  }>()

  const inspectionQuery =
    view === 'applied'
      ? queries.inspectAppliedRuntime()
      : queries.inspectRuntime()
  const inspection = useQuery({
    ...unwrapQueryOptions(inspectionQuery, inspectionQuery.queryFn!),
    refetchOnWindowFocus: false,
    retry: false,
  })
  const snapshot = inspection.data

  // Every rebuild or apply publishes a configuration status change, so the
  // snapshots are refetched on it instead of by hand.
  useEffect(() => {
    const listener = events.configurationStatusChanged
      .listen(() => invalidateSnapshots(queryClient))
      .catch(() => undefined)
    return () => {
      listener.then((unlisten) => unlisten?.())
    }
  }, [queryClient])

  const steps = useMemo(
    () => (snapshot ? buildSteps(snapshot.nodes, snapshot.final_id) : []),
    [snapshot],
  )
  // The step lives in the address, so a rebuild or a switch of snapshot keeps
  // it, and a key that no step has any more falls back to the final one.
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

  const selectStep = (step: string) =>
    navigate({
      to: '.',
      search: (previous) => ({ ...previous, step }),
      replace: true,
      state: keepReturn,
    })

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
        view={view}
        onViewChange={setView}
        layout={layout}
        onLayoutChange={setLayout}
        fieldQuery={fieldQuery}
        onFieldQueryChange={setFieldQuery}
        onCycleMatch={cycleMatch}
        onRefresh={() => invalidateSnapshots(queryClient)}
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
            step={selected}
            steps={steps}
            profiles={profiles}
            targetCore={snapshot.target_core}
            onOpenProfile={openProfile}
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

import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import {
  events,
  queries,
  unwrapQueryOptions,
  type RuntimeInspection,
  type RuntimeInspectionContent,
} from '@nyanpasu/interface'
import { skipToken, useQuery, useQueryClient } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import LogLevelBadge from '../../logs/_modules/log-level-badge'
import ChangedFields from './_modules/changed-fields'
import DiffViewer from './_modules/diff-viewer'
import {
  StepLabel,
  stepParts,
  useOpenProfile,
  useProfileLookup,
} from './_modules/step-label'
import YamlViewer from './_modules/yaml-viewer'

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
})

function RouteComponent() {
  const queryClient = useQueryClient()
  const [appliedView, setAppliedView] = useState(false)
  const inspectionQuery = appliedView
    ? queries.inspectAppliedRuntime()
    : queries.inspectRuntime()
  const inspection = useQuery({
    ...unwrapQueryOptions(inspectionQuery, inspectionQuery.queryFn!),
    refetchOnWindowFocus: false,
    retry: false,
  })

  // Every rebuild or apply publishes a configuration status change, so the
  // snapshots are refetched on it instead of by hand.
  useEffect(() => {
    const listener = events.configurationStatusChanged
      .listen(() => {
        for (const query of [
          queries.inspectRuntime(),
          queries.inspectAppliedRuntime(),
        ]) {
          queryClient.invalidateQueries({ queryKey: query.queryKey })
        }
      })
      .catch(() => undefined)
    return () => {
      listener.then((unlisten) => unlisten?.())
    }
  }, [queryClient])

  return (
    <section className="@container flex min-w-0 flex-1 flex-col gap-4 p-4 pt-0">
      <header className="bg-mixed-background sticky top-0 z-50 flex items-center justify-between gap-4 py-4">
        <h1 className="text-lg font-bold">{m.inspect_title()}</h1>
        <label className="flex items-center gap-2 text-sm">
          {m.inspect_last_applied()}
          <Switch checked={appliedView} onCheckedChange={setAppliedView} />
        </label>
      </header>
      {inspection.isPending && <p role="status">{m.inspect_loading()}</p>}
      {inspection.isError && (
        <p role="alert">
          {m.inspect_error()} {String(inspection.error)}
        </p>
      )}
      {!inspection.isError && inspection.data === null && (
        <p role="status">{m.inspect_empty()}</p>
      )}
      {!inspection.isError && inspection.data && (
        <SnapshotBrowser
          key={inspection.data.snapshot_id}
          snapshot={inspection.data}
        />
      )}
    </section>
  )
}

function SnapshotBrowser({ snapshot }: { snapshot: RuntimeInspection }) {
  const [showAll, setShowAll] = useState(false)
  const [selectedId, setSelectedId] = useState<number>()
  const [view, setView] = useState('diff')
  const profiles = useProfileLookup()
  const openProfile = useOpenProfile(profiles)
  const nodes = showAll
    ? snapshot.nodes
    : snapshot.nodes.filter(
        (node) => node.has_logs || (node.changed_fields?.length ?? 0) > 0,
      )
  const selected = nodes.find((node) => node.id === selectedId) ?? nodes[0]
  const contentQuery = selected
    ? queries.inspectRuntimeNode(snapshot.snapshot_id, selected.id)
    : null
  const content = useQuery<RuntimeInspectionContent>({
    queryKey: contentQuery?.queryKey ?? [
      'runtime-inspection-node',
      snapshot.snapshot_id,
      selected?.id,
    ],
    queryFn: contentQuery
      ? unwrapQueryOptions(contentQuery, contentQuery.queryFn!).queryFn
      : skipToken,
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: 0,
  })

  return (
    <>
      <p className="text-on-surface-variant text-sm">
        {snapshot.effective_pending
          ? m.inspect_effective_pending()
          : snapshot.applied
            ? m.inspect_applied()
            : m.inspect_generated()}{' '}
        · {snapshot.target_core} · {m.inspect_revision()} {snapshot.revision}
      </p>
      <div className="grid min-w-0 gap-4 @[40rem]:grid-cols-[minmax(12rem,1fr)_minmax(0,3fr)]">
        <div className="flex min-w-0 flex-col gap-3">
          <label className="flex items-center justify-between gap-2 text-sm">
            {m.inspect_show_all()}
            <Switch checked={showAll} onCheckedChange={setShowAll} />
          </label>
          <nav
            aria-label={m.inspect_steps()}
            className="flex max-h-[65vh] flex-col gap-1 overflow-auto"
          >
            {nodes.map((node) => (
              <button
                key={node.id}
                type="button"
                aria-current={selected?.id === node.id ? 'step' : undefined}
                onClick={() => setSelectedId(node.id)}
                className="hover:bg-surface-variant aria-[current=step]:bg-secondary-container focus-visible:outline-primary rounded-lg p-3 text-left text-sm focus-visible:outline-2"
              >
                <span className="text-on-surface-variant mr-2">
                  #{node.id + 1}
                </span>
                <StepLabel
                  parts={stepParts(node.tag)}
                  profiles={profiles}
                  onOpenProfile={openProfile}
                />
              </button>
            ))}
          </nav>
        </div>
        {!selected && (
          <p role="status" className="text-on-surface-variant text-sm">
            {m.inspect_filtered_empty()}
          </p>
        )}
        {selected && (
          <div className="flex min-w-0 flex-col gap-3">
            <h2 className="font-medium">
              #{selected.id + 1}{' '}
              <StepLabel
                parts={stepParts(selected.tag)}
                profiles={profiles}
                onOpenProfile={openProfile}
              />
            </h2>
            <ChangedFields key={selected.id} fields={selected.changed_fields} />
            {showAll && selected.next.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <span>{m.inspect_next()}</span>
                {selected.next.map((id) => (
                  <Button key={id} onClick={() => setSelectedId(id)}>
                    #{id + 1}
                  </Button>
                ))}
              </div>
            )}
            {content.isPending && <p role="status">{m.inspect_loading()}</p>}
            {content.isError && (
              <p role="alert">
                {m.inspect_content_error()} {String(content.error)}
              </p>
            )}
            {content.isSuccess && (
              <>
                <SegmentedButton
                  className="max-w-sm"
                  size="sm"
                  value={view}
                  onValueChange={(value) => {
                    if (value) setView(value)
                  }}
                >
                  <SegmentedButtonItem value="yaml">YAML</SegmentedButtonItem>
                  <SegmentedButtonItem value="diff">
                    {m.inspect_diff()}
                  </SegmentedButtonItem>
                </SegmentedButton>
                {view === 'yaml' ? (
                  <YamlViewer
                    code={content.data.yaml}
                    label={m.inspect_yaml()}
                  />
                ) : content.data.diff ? (
                  <>
                    <p className="text-on-surface-variant text-sm">
                      {m.inspect_diff_description({
                        step: content.data.diff.parent_id + 1,
                      })}
                    </p>
                    <DiffViewer hunks={content.data.diff.hunks} />
                  </>
                ) : (
                  <p role="status" className="text-on-surface-variant text-sm">
                    {m.inspect_diff_independent()}
                  </p>
                )}
                <section className="mt-3 flex flex-col gap-2">
                  <h3 className="text-sm font-medium">{m.inspect_logs()}</h3>
                  {content.data.logs.length === 0 ? (
                    <p className="text-on-surface-variant text-sm">
                      {m.inspect_no_logs()}
                    </p>
                  ) : (
                    <ol className="bg-surface divide-outline-variant/50 max-h-64 divide-y overflow-auto rounded-lg">
                      {content.data.logs.map((log, index) => (
                        <li
                          key={index}
                          className="flex items-start gap-3 px-3 py-2"
                        >
                          <LogLevelBadge>{log.level}</LogLevelBadge>
                          <span className="text-on-surface min-w-0 flex-1 font-mono text-xs leading-relaxed [overflow-wrap:anywhere] whitespace-pre-wrap">
                            {log.message}
                          </span>
                        </li>
                      ))}
                    </ol>
                  )}
                </section>
              </>
            )}
          </div>
        )}
      </div>
    </>
  )
}

import { useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import {
  commands,
  unwrapResult,
  type ConfigExecutionRole,
  type OperatorTag,
  type RuntimeInspection,
} from '@nyanpasu/interface'
import { skipToken, useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import DiffViewer from './_modules/diff-viewer'
import YamlViewer from './_modules/yaml-viewer'

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
})

function roleLabel(role: ConfigExecutionRole): string {
  switch (role.kind) {
    case 'selected':
      return m.inspect_selected()
    case 'composition_base':
      return `${m.inspect_base()} → ${role.data.composition_id}`
    case 'composition_contributor':
      return `${m.inspect_contributor()} ${role.data.contributor_index + 1} → ${role.data.composition_id}`
  }
}

function stepLabel(tag: OperatorTag): string {
  switch (tag.kind) {
    case 'bare_root':
      return m.inspect_bare()
    case 'file_config_root':
      return `${m.inspect_file()} · ${tag.data.profile_id} (${roleLabel(tag.data.role)})`
    case 'composition_root':
      return `${m.inspect_composition()} · ${tag.data.profile_id}`
    case 'extend_proxies_step':
      return `${m.inspect_extend()} · ${tag.data.contributor_profile_id} → ${tag.data.composition_id}`
    case 'scoped_transform':
      return `${m.inspect_scoped()} · ${tag.data.transform_profile_id} → ${tag.data.host_profile_id} (${roleLabel(tag.data.role)})`
    case 'global_transform':
      return `${m.inspect_global()} · ${tag.data.transform_profile_id}`
    case 'builtin_transform':
      return `${m.inspect_builtin()} · ${tag.data.name}`
    case 'builtin_step':
      switch (tag.data.step) {
        case 'guard_overrides':
          return m.inspect_overrides()
        case 'whitelist_field_filter':
          return m.inspect_filter()
        case 'finalizing':
          return m.inspect_finalizing()
      }
  }
}

function RouteComponent() {
  const inspection = useQuery({
    queryKey: ['runtime-inspection'],
    queryFn: async () => unwrapResult(await commands.inspectRuntime()),
    refetchOnWindowFocus: false,
    retry: false,
  })

  return (
    <section className="@container flex min-w-0 flex-1 flex-col gap-4 p-4 md:p-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">{m.inspect_title()}</h1>
          <p className="text-on-surface-variant mt-1 text-sm">
            {m.inspect_description()}
          </p>
        </div>
        <Button
          className="shrink-0"
          onClick={() => inspection.refetch()}
          loading={inspection.isFetching}
        >
          {m.inspect_refresh()}
        </Button>
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
  const nodes = showAll
    ? snapshot.nodes
    : snapshot.nodes.filter(
        (node) => node.has_logs || (node.changed_fields?.length ?? 0) > 0,
      )
  const selected = nodes.find((node) => node.id === selectedId) ?? nodes[0]
  const content = useQuery({
    queryKey: ['runtime-inspection-node', snapshot.snapshot_id, selected?.id],
    queryFn: selected
      ? async () =>
          unwrapResult(
            await commands.inspectRuntimeNode(
              snapshot.snapshot_id,
              selected.id,
            ),
          )
      : skipToken,
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: 0,
  })

  return (
    <>
      <p className="text-on-surface-variant text-sm">
        {m.inspect_generated()} · {snapshot.target_core} ·{' '}
        {m.inspect_revision()} {snapshot.revision}
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
                {stepLabel(node.tag)}
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
              #{selected.id + 1} {stepLabel(selected.tag)}
            </h2>
            <div className="text-on-surface-variant text-sm">
              {m.inspect_fields()}:{' '}
              {selected.changed_fields === null
                ? m.inspect_no_baseline()
                : selected.changed_fields.length === 0
                  ? m.inspect_unchanged()
                  : selected.changed_fields.join(', ')}
            </div>
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
                      {m.inspect_diff_description()} #
                      {content.data.diff.parent_id + 1}
                    </p>
                    <DiffViewer hunks={content.data.diff.hunks} />
                  </>
                ) : (
                  <p role="status" className="text-on-surface-variant text-sm">
                    {m.inspect_diff_independent()}
                  </p>
                )}
                <h3 className="text-sm font-medium">{m.inspect_logs()}</h3>
                {content.data.logs.length === 0 ? (
                  <p className="text-on-surface-variant text-sm">
                    {m.inspect_no_logs()}
                  </p>
                ) : (
                  <pre className="bg-surface max-h-48 overflow-auto rounded-lg p-3 text-xs break-words whitespace-pre-wrap">
                    {content.data.logs
                      .map((log) => `[${log.level}] ${log.message}`)
                      .join('\n')}
                  </pre>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </>
  )
}

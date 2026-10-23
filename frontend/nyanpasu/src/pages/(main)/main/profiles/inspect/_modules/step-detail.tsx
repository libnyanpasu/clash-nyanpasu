import { useMemo, useRef, useState, type KeyboardEvent } from 'react'
import AnimatedTabs, { AnimatedTabsItem } from '@nyanpasu/ui/animated-tabs'
import { Button } from '@nyanpasu/ui/button'
import { ErrorMessage } from '@/components/error-message'
import {
  StepLabel,
  stepParts,
  stepSubject,
  type ProfileLookup,
} from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import { queries } from '@/services/rpc'
import { unwrapQueryOptions } from '@nyanpasu/query'
import type { RuntimeInspectionContent } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import LogLevelBadge from '../../../logs/_modules/log-level-badge'
import ChangedFields from './changed-fields'
import DiffViewer from './diff-viewer'
import { FindBar, isFindShortcut } from './find-bar'
import { stageLabel, stepHeadline } from './inspect-labels'
import { FINAL_KEY, type Step } from './inspect-steps'
import {
  diffLines,
  findLines,
  locateFieldLine,
  type FieldJump,
} from './text-find'
import YamlViewer from './yaml-viewer'

export type InspectTab = 'changes' | 'output' | 'logs'

/** The final configuration is read as it is; every other step by its changes. */
export function defaultTab(step: Step): InspectTab {
  return step.key === FINAL_KEY ? 'output' : 'changes'
}

/**
 * One step: what it is, which fields it changed, and its changes, output and
 * logs. The page keys it by step, so its find state starts over with a step.
 */
export default function StepDetail({
  snapshotId,
  step,
  steps,
  profiles,
  onOpenProfile,
  tab,
  onTabChange,
  fieldQuery,
}: {
  snapshotId: string
  step: Step
  /** Every step, whatever the page filters, for the position and the baseline. */
  steps: Step[]
  profiles: ProfileLookup
  onOpenProfile: (id: string) => void
  tab: InspectTab
  onTabChange: (tab: InspectTab) => void
  /** Marks the part of each changed field that matches the toolbar search. */
  fieldQuery: string
}) {
  const [find, setFind] = useState({ open: false, query: '', active: 0 })
  const [jump, setJump] = useState<FieldJump>()
  const jumpCount = useRef(0)
  const findInputRef = useRef<HTMLInputElement>(null)

  const contentQuery = queries.inspectRuntimeNode(snapshotId, step.node.id)
  const content = useQuery<RuntimeInspectionContent>({
    queryKey: contentQuery.queryKey,
    queryFn: unwrapQueryOptions(contentQuery, contentQuery.queryFn!).queryFn,
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: 0,
  })
  const data = content.data

  const yamlLines = useMemo(() => data?.yaml.split('\n') ?? [], [data?.yaml])
  const diffTexts = useMemo(
    () => (data?.diff ? diffLines(data.diff.hunks) : []),
    [data?.diff],
  )
  // Where each changed field sits in the output; a removed field is not there.
  const fieldLines = useMemo(
    () =>
      new Map(
        (step.node.changed_fields ?? []).map((field) => [
          field,
          locateFieldLine(yamlLines, field),
        ]),
      ),
    [step.node.changed_fields, yamlLines],
  )

  const searchable = tab !== 'logs'
  const total = !find.query
    ? 0
    : findLines(tab === 'output' ? yamlLines : diffTexts, find.query).length
  const active = Math.min(find.active, Math.max(total - 1, 0))

  const position = steps.findIndex((item) => item.key === step.key) + 1
  const subject = stepSubject(step.node.tag)
  const profileId =
    typeof subject === 'object' && profiles.has(subject.profileId)
      ? subject.profileId
      : undefined
  const parentStep = steps.find(
    (item) => item.node.id === data?.diff?.parent_id,
  )

  const changeTab = (next: InspectTab) => {
    setJump(undefined)
    setFind((state) => ({ ...state, active: 0 }))
    onTabChange(next)
  }

  const openFind = () => {
    setFind((state) => ({ ...state, open: true }))
    // The bar may be mounting; wait a frame for the input.
    requestAnimationFrame(() => {
      findInputRef.current?.focus()
      findInputRef.current?.select()
    })
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (!searchable || !isFindShortcut(event)) return
    event.preventDefault()
    openFind()
  }

  return (
    <section
      data-slot="inspect-step-detail"
      data-key={step.key}
      className="flex min-h-0 min-w-0 flex-col gap-3 px-4 pb-4 md:px-6"
    >
      <header className="flex shrink-0 flex-col gap-2">
        <p
          data-slot="inspect-step-position"
          className="text-on-surface-variant text-xs font-medium tracking-wide uppercase"
        >
          {m.inspect_step_position({
            stage: stageLabel(step.stage),
            n: position,
            total: steps.length,
          })}
        </p>

        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <h2 className="min-w-0 text-lg font-medium [overflow-wrap:anywhere]">
            {step.key === FINAL_KEY ? (
              stepHeadline(step, profiles)
            ) : (
              <StepLabel
                parts={stepParts(step.node.tag)}
                profiles={profiles}
                onOpenProfile={onOpenProfile}
              />
            )}
          </h2>

          <div className="ml-auto flex flex-wrap items-center gap-2">
            {profileId && (
              <Button type="button" onClick={() => onOpenProfile(profileId)}>
                {m.inspect_open_profile()}
              </Button>
            )}

            {step.stage === 'global' && (
              <Button asChild>
                <Link to="/main/profiles/global">
                  {m.inspect_edit_in_global()}
                </Link>
              </Button>
            )}
          </div>
        </div>

        <ChangedFields
          key={step.key}
          fields={step.node.changed_fields}
          query={fieldQuery}
          canSelect={(field) => fieldLines.get(field) != null}
          onSelect={(field) => {
            const lines = fieldLines.get(field)
            if (!lines) return
            setJump({ field, ...lines, nonce: ++jumpCount.current })
          }}
        />
      </header>

      <AnimatedTabs
        className="shrink-0 self-start"
        activeTab={tab}
        onChange={(next) => changeTab(next as InspectTab)}
      >
        <AnimatedTabsItem value="changes">
          {m.inspect_tab_changes()}
        </AnimatedTabsItem>
        <AnimatedTabsItem value="output">
          {m.inspect_tab_output()}
        </AnimatedTabsItem>
        <AnimatedTabsItem value="logs">
          {m.inspect_tab_logs()}
          {data && data.logs.length > 0 && ` (${data.logs.length})`}
        </AnimatedTabsItem>
      </AnimatedTabs>

      <div
        role="tabpanel"
        data-slot="inspect-step-panel"
        data-tab={tab}
        className="flex min-h-0 flex-1 flex-col gap-2"
        onKeyDown={handleKeyDown}
      >
        {content.isPending && (
          <p role="status" className="text-on-surface-variant text-sm">
            {m.inspect_loading()}
          </p>
        )}

        {content.isError && (
          <p role="alert" className="text-sm">
            {m.inspect_content_error()} <ErrorMessage error={content.error} />
          </p>
        )}

        {data && (
          <>
            {find.open && searchable && (
              <FindBar
                className="shrink-0"
                inputRef={findInputRef}
                query={find.query}
                onQueryChange={(query) =>
                  setFind((state) => ({ ...state, query, active: 0 }))
                }
                total={total}
                active={active}
                onActiveChange={(next) =>
                  setFind((state) => ({ ...state, active: next }))
                }
                onClose={() => setFind((state) => ({ ...state, open: false }))}
                status={
                  total === 0
                    ? m.inspect_find_none()
                    : m.inspect_find_status({ current: active + 1, total })
                }
                label={m.inspect_find_label()}
                clearLabel={m.inspect_find_clear()}
                previousLabel={m.inspect_find_previous()}
                nextLabel={m.inspect_find_next()}
                closeLabel={m.inspect_find_close()}
              />
            )}

            {tab === 'output' && (
              <YamlViewer
                code={data.yaml}
                label={m.inspect_yaml()}
                cacheKey={[snapshotId, step.node.id]}
                find={find.open ? { query: find.query, active } : undefined}
                jump={jump}
              />
            )}

            {tab === 'changes' &&
              (data.diff ? (
                <>
                  <p className="text-on-surface-variant shrink-0 text-sm">
                    {m.inspect_diff_description({
                      step: parentStep
                        ? stepHeadline(parentStep, profiles)
                        : `#${data.diff.parent_id + 1}`,
                    })}
                  </p>

                  <DiffViewer
                    hunks={data.diff.hunks}
                    find={find.open ? { query: find.query, active } : undefined}
                    jump={jump}
                  />
                </>
              ) : (
                <p role="status" className="text-on-surface-variant text-sm">
                  {m.inspect_diff_independent()}
                </p>
              ))}

            {tab === 'logs' &&
              (data.logs.length === 0 ? (
                <p role="status" className="text-on-surface-variant text-sm">
                  {m.inspect_no_logs()}
                </p>
              ) : (
                <ol
                  aria-label={m.inspect_logs()}
                  className={cn(
                    'bg-surface divide-outline-variant/50 min-h-0 flex-1',
                    'divide-y overflow-auto rounded-lg',
                  )}
                >
                  {data.logs.map((log, index) => (
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
              ))}
          </>
        )}
      </div>
    </section>
  )
}

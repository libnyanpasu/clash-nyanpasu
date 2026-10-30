import {
  useDeferredValue,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import AnimatedTabs, { AnimatedTabsItem } from '@nyanpasu/ui/animated-tabs'
import { ErrorMessage } from '@/components/error-message'
import {
  StepLabel,
  stepParts,
  stepSubject,
  type ProfileLookup,
} from '@/components/profile-label'
import { usePageEntered } from '@/components/router/animated-outlet'
import { m } from '@/paraglide/messages'
import { queries } from '@/services/rpc'
import { unwrapQueryOptions } from '@nyanpasu/query'
import type { RuntimeInspectionContent } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { CompactButton } from '../../_modules/compact-controls'
import LogLevelBadge from '../../../logs/_modules/log-level-badge'
import ChangedFields from './changed-fields'
import DiffViewer from './diff-viewer'
import { FindBar, isFindShortcut } from './find-bar'
import { stageLabel, stepHeadline, stepSub } from './inspect-labels'
import { FINAL_KEY, type Step } from './inspect-steps'
import StepPanelSkeleton from './step-panel-skeleton'
import { diffLines, fieldLocator, findLines, type FieldJump } from './text-find'
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
  targetCore,
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
  targetCore: string
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
  const fieldLines = useMemo(() => {
    const locate = fieldLocator(yamlLines)
    return new Map(
      (step.node.changed_fields ?? []).map((field) => [field, locate(field)]),
    )
  }, [step.node.changed_fields, yamlLines])

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

  const changedCount = step.changedCount ?? 0
  // The note for a step that has nothing to compare, or nothing that differs.
  const note =
    step.key === FINAL_KEY
      ? m.inspect_note_final()
      : step.status === 'core'
        ? m.inspect_note_core()
        : data?.diff
          ? m.inspect_note_unchanged()
          : m.inspect_diff_independent()

  return (
    <section
      data-slot="inspect-step-detail"
      data-key={step.key}
      className="flex min-h-0 min-w-0 flex-col"
    >
      <header className="flex shrink-0 flex-col gap-2.5 px-5 pt-4 pb-3 shadow-[inset_0_-1px_0_var(--color-outline-variant)]">
        <p
          data-slot="inspect-step-position"
          className="text-primary text-xs font-semibold tracking-[0.03em] uppercase"
        >
          {m.inspect_step_position({
            stage: stageLabel(step.stage),
            n: position,
            total: steps.length,
          })}
        </p>

        <div className="flex items-center gap-2.5">
          <h2 className="text-on-surface min-w-0 flex-1 truncate text-xl leading-7 font-semibold">
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

          {profileId && (
            <CompactButton onClick={() => onOpenProfile(profileId)}>
              {m.inspect_open_profile()}
            </CompactButton>
          )}

          {step.stage === 'global' && (
            <CompactButton tone="tonal" type={undefined} asChild>
              <Link to="/main/profiles/global">
                {m.inspect_edit_in_global()}
              </Link>
            </CompactButton>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-2.5">
          <AnimatedTabs
            variant="segment"
            size="sm"
            className="h-8 shrink-0 text-[13px]"
            activeTab={tab}
            onChange={(next) => changeTab(next as InspectTab)}
          >
            <AnimatedTabsItem value="changes" className="px-3.5">
              {m.inspect_tab_changes()}
            </AnimatedTabsItem>
            <AnimatedTabsItem value="output" className="px-3.5">
              {m.inspect_tab_output()}
            </AnimatedTabsItem>
            <AnimatedTabsItem value="logs" className="px-3.5">
              {data && data.logs.length > 0
                ? m.inspect_tab_logs_count({ count: data.logs.length })
                : m.inspect_tab_logs()}
            </AnimatedTabsItem>
          </AnimatedTabs>

          <span
            data-slot="inspect-step-summary"
            className="text-on-surface-variant min-w-0 text-[13px]"
          >
            {changedCount > 0
              ? m.inspect_summary_changed({ count: changedCount })
              : stepSub(step, steps, profiles, targetCore)}
          </span>
        </div>

        {tab === 'changes' && (
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
        )}
      </header>

      <div
        role="tabpanel"
        data-slot="inspect-step-panel"
        data-tab={tab}
        className="bg-code flex min-h-0 flex-1 flex-col"
        onKeyDown={handleKeyDown}
      >
        {content.isPending && <StepPanelSkeleton />}

        {content.isError && (
          <p role="alert" className="p-6 text-[13px]">
            {m.inspect_content_error()} <ErrorMessage error={content.error} />
          </p>
        )}

        {data && (
          <DeferredPanel tab={tab}>
            {find.open && searchable && (
              <FindBar
                className="mx-5 mt-3 mb-1 shrink-0"
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
              (data.diff && data.diff.hunks.length > 0 ? (
                <>
                  <p className="text-on-surface-variant shrink-0 px-5 pt-2.5 pb-1.5 text-xs">
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
                <p
                  role="status"
                  className="text-on-surface-variant p-6 text-[13px]"
                >
                  {note}
                </p>
              ))}

            {tab === 'logs' &&
              (data.logs.length === 0 ? (
                <p
                  role="status"
                  className="text-on-surface-variant p-6 text-[13px]"
                >
                  {m.inspect_no_logs()}
                </p>
              ) : (
                <ol
                  aria-label={m.inspect_logs()}
                  className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-auto p-3"
                >
                  {data.logs.map((log, index) => (
                    <li
                      key={index}
                      className="bg-surface flex items-start gap-2.5 rounded-xl px-3 py-2.5 text-[13px] leading-[18px]"
                    >
                      <LogLevelBadge className="h-5 rounded-md px-[7px] py-0 font-bold">
                        {log.level}
                      </LogLevelBadge>

                      <span className="text-on-surface min-w-0 flex-1 [overflow-wrap:anywhere] whitespace-pre-wrap">
                        {log.message}
                      </span>
                    </li>
                  ))}
                </ol>
              ))}
          </DeferredPanel>
        )}
      </div>
    </section>
  )
}

/**
 * The document of the panel's tab, once the page has slid in and a render
 * after the tab changed; a skeleton until then. The tabs' indicator measures
 * the page as it moves, and a document of thousands of lines that mounts in
 * the same commit is laid out in full for it.
 */
function DeferredPanel({
  tab,
  children,
}: {
  tab: InspectTab
  children: ReactNode
}) {
  const entered = usePageEntered()
  const shownTab = useDeferredValue(tab)

  return entered && shownTab === tab ? children : <StepPanelSkeleton />
}

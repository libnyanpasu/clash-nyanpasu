import type { OperatorTag, RuntimeInspectionNode } from '@nyanpasu/rpc/types'

// Pure model of the Inspect page. It has no message imports on purpose: the
// view maps stages and statuses to text.

export type Stage = 'source' | 'profile' | 'global' | 'nyanpasu' | 'result'

export const STAGES: readonly Stage[] = [
  'source',
  'profile',
  'global',
  'nyanpasu',
  'result',
]

export type StepStatus =
  'start' | 'changed' | 'unchanged' | 'warn' | 'error' | 'final' | 'core'

export type Step = {
  /** Stable across rebuilds, unlike the node id. */
  key: string
  node: RuntimeInspectionNode
  stage: Stage
  /** 1 for the steps shown under their composition. */
  depth: 0 | 1
  status: StepStatus
  /** The number of changed fields; null when there is no comparison baseline. */
  changedCount: number | null
}

export const FINAL_KEY = 'final'

/** The stage a node belongs to. The final node wins over its own tag. */
export function stageOf(node: RuntimeInspectionNode, finalId: number): Stage {
  if (node.id === finalId) return 'result'
  const { tag } = node
  switch (tag.kind) {
    case 'bare_root':
    case 'file_config_root':
    case 'composition_root':
    case 'extend_proxies_step':
      return 'source'
    case 'scoped_transform':
      return 'profile'
    case 'global_transform':
      return 'global'
    case 'builtin_transform':
      return 'nyanpasu'
    case 'builtin_step':
      return tag.data.step === 'core_controller' ? 'result' : 'nyanpasu'
  }
}

function baseKey(node: RuntimeInspectionNode, finalId: number): string {
  if (node.id === finalId) return FINAL_KEY
  const { tag } = node
  switch (tag.kind) {
    case 'bare_root':
      return 'root:bare'
    case 'file_config_root':
    case 'composition_root':
      return `root:${tag.data.profile_id}`
    case 'extend_proxies_step':
      return `extend:${tag.data.composition_id}:${tag.data.contributor_index}`
    case 'scoped_transform': {
      const { host_profile_id, transform_profile_id, step_index, role } =
        tag.data
      const key = `scoped:${host_profile_id}:${transform_profile_id}:${step_index}`
      return role.kind === 'selected'
        ? key
        : `${key}@${role.data.composition_id}`
    }
    case 'global_transform':
      return `global:${tag.data.transform_profile_id}:${tag.data.step_index}`
    case 'builtin_transform':
      return `builtin-transform:${tag.data.step_index}`
    case 'builtin_step':
      return tag.data.step === 'core_controller'
        ? 'core'
        : `builtin:${tag.data.step}`
  }
}

/** The composition a source step is shown under, if any. */
function compositionOf(tag: OperatorTag): string | undefined {
  switch (tag.kind) {
    case 'extend_proxies_step':
      return tag.data.composition_id
    case 'file_config_root':
      return tag.data.role.kind === 'selected'
        ? undefined
        : tag.data.role.data.composition_id
    default:
      return undefined
  }
}

function isRoot(tag: OperatorTag) {
  return (
    tag.kind === 'bare_root' ||
    tag.kind === 'file_config_root' ||
    tag.kind === 'composition_root'
  )
}

/**
 * Failures win over everything else, so a step that logged an error is never
 * shown as unchanged.
 */
export function stepStatus(
  node: RuntimeInspectionNode,
  finalId: number,
): StepStatus {
  if (node.max_log_level === 'error') return 'error'
  if (node.max_log_level === 'warn') return 'warn'
  if (node.id === finalId) return 'final'
  if (
    node.tag.kind === 'builtin_step' &&
    node.tag.data.step === 'core_controller'
  )
    return 'core'
  if (node.changed_fields === null && isRoot(node.tag)) return 'start'
  return (node.changed_fields?.length ?? 0) > 0 ? 'changed' : 'unchanged'
}

/**
 * The steps in display order: by stage, then by node id. The steps that
 * belong to a composition follow it, one level in.
 */
export function buildSteps(
  nodes: RuntimeInspectionNode[],
  finalId: number,
): Step[] {
  const seen = new Map<string, number>()
  const make = (node: RuntimeInspectionNode): Step => {
    const base = baseKey(node, finalId)
    const count = (seen.get(base) ?? 0) + 1
    seen.set(base, count)
    return {
      key: count === 1 ? base : `${base}~${count}`,
      node,
      stage: stageOf(node, finalId),
      depth: 0,
      status: stepStatus(node, finalId),
      changedCount: node.changed_fields?.length ?? null,
    }
  }

  // Keys are assigned in node order, whatever the display order is.
  const steps = [...nodes].sort((a, b) => a.id - b.id).map(make)

  const byStage = new Map<Stage, Step[]>(STAGES.map((stage) => [stage, []]))
  for (const step of steps) byStage.get(step.stage)!.push(step)

  return STAGES.flatMap((stage) => {
    const group = byStage.get(stage)!
    if (stage !== 'source') return group

    const compositions = new Set(
      group.flatMap(({ node }) =>
        node.tag.kind === 'composition_root' ? [node.tag.data.profile_id] : [],
      ),
    )
    const nested = (step: Step) => {
      const id = compositionOf(step.node.tag)
      return id !== undefined && compositions.has(id) ? id : undefined
    }
    return group.flatMap((step) => {
      // A nested step is shown under its composition, and keeps its own place
      // only when that composition is missing.
      if (nested(step) !== undefined) return []
      if (step.node.tag.kind !== 'composition_root') return [step]
      const id = step.node.tag.data.profile_id
      const children = group
        .filter((child) => nested(child) === id)
        .map((child): Step => ({ ...child, depth: 1 }))
      return [step, ...children]
    })
  })
}

/**
 * The "Only steps that changed something" filter. It keeps the whole Source
 * and Result stages, the selected step and every step that changed fields or
 * logged a warning or an error.
 */
export function filterSteps(
  steps: Step[],
  options: { onlyChanged: boolean; selectedKey?: string },
): Step[] {
  if (!options.onlyChanged) return steps
  return steps.filter(
    (step) =>
      step.stage === 'source' ||
      step.stage === 'result' ||
      step.key === options.selectedKey ||
      (step.changedCount ?? 0) > 0 ||
      step.status === 'warn' ||
      step.status === 'error',
  )
}

/** The step a `?step=` value names; an unknown or missing key means Final. */
export function resolveStep(
  steps: Step[],
  key: string | undefined,
): Step | undefined {
  return (
    steps.find((step) => step.key === key) ??
    steps.find((step) => step.key === FINAL_KEY) ??
    steps.at(-1)
  )
}

/**
 * Whether a step changed a field the query points at. The query is a path,
 * so `dns.nameserver` finds a step that changed `dns`.
 */
export function matchField(step: Step, query: string): boolean {
  const needle = query.trim().toLowerCase()
  if (!needle) return false
  return (
    step.node.changed_fields?.some((field) => {
      const name = field.toLowerCase()
      return (
        name.includes(needle) ||
        needle.startsWith(`${name}.`) ||
        needle.startsWith(`${name}[`)
      )
    }) ?? false
  )
}

/** The transforms that run one after the other share a chain. */
function chainOf(tag: OperatorTag): string | undefined {
  switch (tag.kind) {
    case 'scoped_transform':
      return `scoped:${tag.data.host_profile_id}:${
        tag.data.role.kind === 'selected'
          ? ''
          : tag.data.role.data.composition_id
      }`
    case 'global_transform':
      return 'global'
    case 'builtin_transform':
      return 'builtin-transform'
    default:
      return undefined
  }
}

/**
 * Where a transform stands in its chain, in display order, for "1 of 2".
 * Steps that are not transforms have no position.
 */
export function chainPosition(
  steps: Step[],
  step: Step,
): { n: number; total: number } | undefined {
  const chain = chainOf(step.node.tag)
  if (chain === undefined) return undefined
  const members = steps.filter((item) => chainOf(item.node.tag) === chain)
  return { n: members.indexOf(step) + 1, total: members.length }
}

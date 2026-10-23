import {
  partsText,
  stepKind,
  stepSubject,
  type ProfileLookup,
} from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import { PROFILE_TYPE_NAMES } from '../../_modules/consts'
import { profileTypeOf } from '../../_modules/utils'
import {
  chainPosition,
  FINAL_KEY,
  type Stage,
  type Step,
  type StepStatus,
} from './inspect-steps'

/** The heading of a stage, in the rail and in the step kicker. */
export function stageLabel(stage: Stage): string {
  switch (stage) {
    case 'source':
      return m.inspect_stage_source()
    case 'profile':
      return m.inspect_stage_profile()
    case 'global':
      return m.inspect_stage_global()
    case 'nyanpasu':
      return m.inspect_stage_nyanpasu()
    case 'result':
      return m.inspect_stage_result()
  }
}

/** What a status icon says to a screen reader. */
export function statusLabel(status: StepStatus): string {
  switch (status) {
    case 'start':
      return m.inspect_status_start()
    case 'changed':
      return m.inspect_status_changed()
    case 'unchanged':
      return m.inspect_status_unchanged()
    case 'warn':
      return m.inspect_status_warn()
    case 'error':
      return m.inspect_status_error()
    case 'final':
      return m.inspect_status_final()
    case 'core':
      return m.inspect_status_core()
  }
}

/** What the row is called: the thing the step runs, else the kind of step. */
export function stepHeadline(step: Step, profiles: ProfileLookup) {
  if (step.key === FINAL_KEY) return m.inspect_final_config()
  const subject = stepSubject(step.node.tag)
  return subject === undefined
    ? stepKind(step.node.tag)
    : partsText([subject], profiles)
}

/** The line under a row's title: what the step is, or what it does. */
export function stepSub(
  step: Step,
  steps: Step[],
  profiles: ProfileLookup,
  targetCore: string,
) {
  const { tag } = step.node
  if (step.key === FINAL_KEY) return m.inspect_step_desc_final()

  if (tag.kind === 'builtin_step') {
    switch (tag.data.step) {
      case 'field_filter':
        return m.inspect_step_desc_field_filter()
      case 'keep_unmanaged':
        return m.inspect_step_desc_keep_unmanaged()
      case 'managed_settings':
        return m.inspect_step_desc_managed_settings()
      case 'defaults':
        return m.inspect_step_desc_defaults()
      case 'include_all_expansion':
        return m.inspect_step_desc_include_all()
      case 'core_controller':
        return m.inspect_step_desc_core({ core: targetCore })
    }
  }

  const position = chainPosition(steps, step)
  if (!position) return stepKind(tag)

  const profile =
    tag.kind === 'scoped_transform' || tag.kind === 'global_transform'
      ? profiles.get(tag.data.transform_profile_id)
      : undefined
  const type = profile
    ? PROFILE_TYPE_NAMES[profileTypeOf(profile)]
    : stepKind(tag)
  return position.total > 1
    ? m.inspect_step_sub_position({ type, ...position })
    : type
}

/** The note at the end of a row: how much the step changed. */
export function stepMeta(step: Step, matched: boolean) {
  if (matched) return m.inspect_step_matches()
  if (step.key === FINAL_KEY) return undefined
  if (step.status === 'start') return m.inspect_step_start()
  return (step.changedCount ?? 0) > 0
    ? m.inspect_step_fields({ count: step.changedCount! })
    : m.inspect_step_unchanged()
}

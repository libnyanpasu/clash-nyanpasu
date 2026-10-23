import {
  partsText,
  stepKind,
  stepSubject,
  type ProfileLookup,
} from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import {
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

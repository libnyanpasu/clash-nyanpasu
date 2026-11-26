import { profileName, type ProfileLookup } from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import type {
  CandidateInspection_Serialize,
  HaltedStep,
} from '@nyanpasu/rpc/types'

/** What the viewer is told about a kept candidate beside its steps. */
export type CandidateState = Pick<
  CandidateInspection_Serialize,
  'halted' | 'stage'
>

function stageText(stage: CandidateState['stage']) {
  switch (stage) {
    case 'built':
      return undefined
    case 'prechecked':
      return m.inspect_candidate_prechecked()
    case 'applied_unpublished':
      return m.inspect_candidate_applied_unpublished()
  }
}

/** The step a stopped build could not run, named the way its rows are. */
function haltedStepName(step: HaltedStep, profiles: ProfileLookup) {
  switch (step.kind) {
    case 'profile':
    case 'transform':
      return profileName(profiles, step.profile_id)
    case 'builtin_transform':
      return step.name
  }
}

/**
 * How far a kept candidate got, above its steps. A stopped build names the
 * step it could not run and why here, because that step has no output to
 * show.
 */
export default function CandidateNotice({
  candidate,
  profiles,
}: {
  candidate: CandidateState
  profiles: ProfileLookup
}) {
  const stage = stageText(candidate.stage)
  const { halted } = candidate
  if (stage === undefined && halted == null) return null

  return (
    <div
      data-slot="inspect-candidate-notice"
      role="note"
      className="border-outline-variant flex shrink-0 flex-col gap-1 border-b px-4 py-2 text-sm"
    >
      {stage && <p className="text-on-surface-variant">{stage}</p>}

      {halted != null && (
        <p data-slot="inspect-candidate-halted" className="break-words">
          <span className="text-error font-medium">
            {halted.step == null
              ? m.inspect_candidate_halted()
              : m.inspect_candidate_halted_at({
                  step: haltedStepName(halted.step, profiles),
                })}
          </span>{' '}
          {halted.reason}
        </p>
      )}
    </div>
  )
}

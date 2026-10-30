import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import CodeRounded from '~icons/material-symbols/code-rounded'
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded'
import PetsRounded from '~icons/material-symbols/pets-rounded'
import SettingsOutlineRounded from '~icons/material-symbols/settings-outline-rounded'
import { useId } from 'react'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { sourceLabelOf } from '../../_modules/profile-card-features'
import {
  StageLabel,
  StepGroup,
  StepRowIcon,
  StepRowText,
  stepRowVariants,
} from '../../_modules/step-list'
import { TransformChain } from '../../_modules/transform-chain-list'
import type { TransformChainDraft } from '../../_modules/use-transform-chain-draft'
import {
  profileTypeOf,
  type ConfigProfile,
  type TransformProfile,
} from '../../_modules/utils'
import { BUILTIN_STEP_LABELS } from './builtin-steps'
import Column, { ColumnHead } from './column'

const chevronClassName = 'text-on-surface-variant size-5 shrink-0'

/** A dim row that opens the detail page of the config it speaks of. */
function ConfigRow({
  active,
  title,
  sub,
  slot = 'profile-global-pipeline-link-row',
  icon: Icon,
}: {
  active: ConfigProfile
  title: string
  sub?: string
  slot?: string
  icon: typeof CodeRounded
}) {
  return (
    <Link
      to="/main/profiles/$type/detail/$uid"
      params={{ type: profileTypeOf(active), uid: active.uid }}
      data-slot={slot}
      className={stepRowVariants({ interactive: true })}
    >
      <StepRowIcon>
        <Icon />
      </StepRowIcon>

      <StepRowText dim title={title} sub={sub} />

      <ChevronRightRounded className={chevronClassName} aria-hidden />
    </Link>
  )
}

function SourceStage({ active }: { active?: ConfigProfile }) {
  return (
    <StepGroup>
      {active ? (
        <ConfigRow
          active={active}
          icon={DescriptionOutlineRounded}
          title={active.name}
          sub={m.profile_global_source_sub({ kind: sourceLabelOf(active) })}
        />
      ) : (
        <div
          data-slot="profile-global-pipeline-source-empty"
          className={stepRowVariants()}
        >
          <StepRowIcon>
            <DescriptionOutlineRounded />
          </StepRowIcon>

          <StepRowText dim title={m.profile_global_context_none()} />
        </div>
      )}
    </StepGroup>
  )
}

function ProfileTransformsStage({
  active,
  scoped,
}: {
  active?: ConfigProfile
  scoped: TransformProfile[]
}) {
  const empty = scoped.length === 0

  return (
    <StepGroup>
      {active ? (
        <ConfigRow
          active={active}
          icon={CodeRounded}
          slot={
            empty
              ? 'profile-global-pipeline-scoped-empty'
              : 'profile-global-pipeline-scoped'
          }
          title={
            empty
              ? m.profile_global_profile_transforms_empty()
              : m.profile_global_scoped_title({ count: scoped.length })
          }
          sub={
            empty
              ? undefined
              : m.profile_global_scoped_sub({
                  names: scoped.map((profile) => profile.name).join(', '),
                })
          }
        />
      ) : (
        <div
          data-slot="profile-global-pipeline-scoped-empty"
          className={stepRowVariants()}
        >
          <StepRowIcon>
            <CodeRounded />
          </StepRowIcon>

          <StepRowText
            dim
            title={m.profile_global_profile_transforms_empty()}
          />
        </div>
      )}
    </StepGroup>
  )
}

function NyanpasuStage() {
  const labels = Object.values(BUILTIN_STEP_LABELS)

  return (
    <StepGroup>
      <Link
        to="/main/settings/clash/runtime"
        data-slot="profile-global-pipeline-builtin"
        className={stepRowVariants({ interactive: true })}
      >
        <StepRowIcon>
          <PetsRounded />
        </StepRowIcon>

        <StepRowText
          dim
          title={m.profile_global_builtin_title({ count: labels.length })}
          sub={labels.map((label) => label()).join(', ')}
        />

        <SettingsOutlineRounded className={chevronClassName} aria-hidden />
      </Link>
    </StepGroup>
  )
}

/**
 * The stages a config passes through before the core runs it, top to bottom.
 * Only the global stage is editable here; the others show where it sits.
 */
export default function Pipeline({
  active,
  scoped,
  chain,
  traceStepOf,
}: {
  active?: ConfigProfile
  /** The active config's own transforms, resolved. */
  scoped: TransformProfile[]
  chain: TransformChainDraft
  traceStepOf: (uid: string, index: number) => string
}) {
  const headingId = useId()

  return (
    <Column
      data-slot="profile-global-pipeline-panel"
      aria-labelledby={headingId}
    >
      <ColumnHead id={headingId} title={m.profile_global_pipeline_title()}>
        <span className="text-on-surface-variant truncate text-xs font-medium">
          {m.profile_global_pipeline_aside()}
        </span>
      </ColumnHead>

      <div data-slot="profile-global-pipeline-stage">
        <StageLabel index={1}>{m.profile_global_stage_source()}</StageLabel>
        <SourceStage active={active} />
      </div>

      <div data-slot="profile-global-pipeline-stage">
        <StageLabel index={2}>{m.profile_global_stage_profile()}</StageLabel>
        <ProfileTransformsStage active={active} scoped={scoped} />
      </div>

      <div data-slot="profile-global-pipeline-stage">
        <StageLabel index={3} aside={m.profile_global_stage_editable()}>
          {m.profile_global_stage_global()}
        </StageLabel>

        <TransformChain
          variant="step"
          chain={chain}
          traceStepOf={traceStepOf}
          empty={{
            message: m.profile_global_chain_empty(),
            hint: m.profile_global_chain_empty_hint(),
          }}
        />
      </div>

      <div data-slot="profile-global-pipeline-stage">
        <StageLabel index={4}>{m.profile_global_stage_nyanpasu()}</StageLabel>
        <NyanpasuStage />
      </div>
    </Column>
  )
}

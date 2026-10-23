import { type ReactNode } from 'react'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { TransformChain } from '../../_modules/transform-chain-list'
import type { TransformChainDraft } from '../../_modules/use-transform-chain-draft'
import {
  profileTypeOf,
  transformLabelOf,
  type ConfigProfile,
  type TransformProfile,
} from '../../_modules/utils'
import { BUILTIN_STEP_LABELS } from './builtin-steps'
import PipelineStage from './pipeline-stage'
import { RowTitle, rowVariants } from './row'

const configKindLabel = (config: ConfigProfile) =>
  config.config.type === 'file'
    ? m.profile_kind_file()
    : m.profile_kind_composition()

const linkClassName =
  'text-primary hover:bg-primary/8 focus-visible:outline-primary inline-flex w-fit rounded-md px-2 py-1 text-sm font-medium focus-visible:outline-2'

/** A row that opens the detail page of the profile it names. */
function ProfileLinkRow({
  profile,
  type,
}: {
  profile: ConfigProfile | TransformProfile
  type?: string
}) {
  return (
    <Link
      to="/main/profiles/$type/detail/$uid"
      params={{ type: profileTypeOf(profile), uid: profile.uid }}
      data-slot="profile-global-pipeline-link-row"
      className={rowVariants({ interactive: true })}
    >
      <RowTitle name={profile.name} type={type} />
    </Link>
  )
}

function SourceStage({ active }: { active?: ConfigProfile }) {
  if (!active) {
    return (
      <p
        data-slot="profile-global-pipeline-source-empty"
        className="text-on-surface-variant px-3 text-sm"
      >
        {m.profile_global_context_none()}
      </p>
    )
  }

  return <ProfileLinkRow profile={active} type={configKindLabel(active)} />
}

function ProfileTransformsStage({
  active,
  scoped,
}: {
  active?: ConfigProfile
  scoped: TransformProfile[]
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      {scoped.length === 0 ? (
        <p
          data-slot="profile-global-pipeline-scoped-empty"
          className="text-on-surface-variant px-3 text-sm"
        >
          {m.profile_global_profile_transforms_empty()}
        </p>
      ) : (
        <ul
          data-slot="profile-global-pipeline-scoped"
          className="flex min-w-0 flex-col gap-2"
        >
          {scoped.map((profile, index) => (
            <li key={`${profile.uid}-${index}`}>
              <ProfileLinkRow
                profile={profile}
                type={transformLabelOf(profile)}
              />
            </li>
          ))}
        </ul>
      )}

      {active && (
        <Link
          to="/main/profiles/$type/detail/$uid"
          params={{ type: profileTypeOf(active), uid: active.uid }}
          data-slot="profile-global-pipeline-scoped-edit"
          className={linkClassName}
        >
          {m.profile_global_profile_transforms_edit()}
        </Link>
      )}
    </div>
  )
}

function NyanpasuStage() {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <ol
        data-slot="profile-global-pipeline-builtin"
        className="bg-surface-variant/40 flex min-w-0 flex-col gap-1.5 rounded-3xl px-4 py-3"
      >
        {Object.entries(BUILTIN_STEP_LABELS).map(([kind, label]) => (
          <li key={kind} className="text-sm">
            {label()}
          </li>
        ))}
      </ol>

      <Link
        to="/main/settings/clash/runtime"
        data-slot="profile-global-pipeline-runtime-link"
        className={linkClassName}
      >
        {m.settings_label_runtime()}
      </Link>
    </div>
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
}): ReactNode {
  return (
    <ol
      data-slot="profile-global-pipeline"
      className="flex min-w-0 flex-col gap-5 px-1 pb-2"
    >
      <PipelineStage
        index={1}
        name={m.profile_global_stage_source()}
        description={m.profile_global_stage_source_description()}
      >
        <SourceStage active={active} />
      </PipelineStage>

      <PipelineStage
        index={2}
        name={m.profile_global_stage_profile()}
        description={m.profile_global_stage_profile_description()}
      >
        <ProfileTransformsStage active={active} scoped={scoped} />
      </PipelineStage>

      <PipelineStage
        index={3}
        name={m.profile_global_stage_global()}
        description={m.profile_global_transform_hint()}
      >
        <TransformChain chain={chain} traceStepOf={traceStepOf} />
      </PipelineStage>

      <PipelineStage
        index={4}
        name={m.profile_global_stage_nyanpasu()}
        description={m.profile_global_stage_nyanpasu_description()}
      >
        <NyanpasuStage />
      </PipelineStage>
    </ol>
  )
}

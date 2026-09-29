import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import {
  useProfile,
  type ConfigExecutionRole,
  type OperatorTag,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { useNavigate } from '@tanstack/react-router'
import { profileTypeOf } from '../../$type/_modules/utils'

/** A step label: plain text runs and references to profiles by id. */
export type LabelPart = string | { profileId: string }

export type ProfileLookup = Map<string, ProfileItem_Serialize>

const profileTagClassName = cn(
  'bg-tertiary-container text-on-tertiary-container',
  'inline-block max-w-full rounded-md px-1.5 py-0.5 text-xs [overflow-wrap:anywhere]',
)

function roleParts(role: ConfigExecutionRole): LabelPart[] {
  switch (role.kind) {
    case 'selected':
      return [m.inspect_selected()]
    case 'composition_base':
      return [`${m.inspect_base()} → `, { profileId: role.data.composition_id }]
    case 'composition_contributor':
      return [
        `${m.inspect_contributor()} ${role.data.contributor_index + 1} → `,
        { profileId: role.data.composition_id },
      ]
  }
}

/** What kind of step this is, e.g. a profile transform. */
export function stepKind(tag: OperatorTag): string {
  switch (tag.kind) {
    case 'bare_root':
      return m.inspect_bare()
    case 'file_config_root':
      return m.inspect_file()
    case 'composition_root':
      return m.inspect_composition()
    case 'extend_proxies_step':
      return m.inspect_extend()
    case 'scoped_transform':
      return m.inspect_scoped()
    case 'global_transform':
      return m.inspect_global()
    case 'builtin_transform':
      return m.inspect_builtin()
    case 'builtin_step':
      switch (tag.data.step) {
        case 'guard_overrides':
          return m.inspect_overrides()
        case 'whitelist_field_filter':
          return m.inspect_filter()
        case 'include_all_expansion':
          return m.inspect_include_all()
        case 'core_controller':
          return m.inspect_core_controller()
        case 'finalizing':
          return m.inspect_finalizing()
      }
  }
}

/** What the step itself runs, if it runs something named. */
export function stepSubject(tag: OperatorTag): LabelPart | undefined {
  switch (tag.kind) {
    case 'file_config_root':
    case 'composition_root':
      return { profileId: tag.data.profile_id }
    case 'extend_proxies_step':
      return { profileId: tag.data.contributor_profile_id }
    case 'scoped_transform':
    case 'global_transform':
      return { profileId: tag.data.transform_profile_id }
    case 'builtin_transform':
      return tag.data.name
    case 'bare_root':
    case 'builtin_step':
      return undefined
  }
}

/** Where the step's result goes, relative to other steps. */
function stepContext(tag: OperatorTag): LabelPart[] {
  switch (tag.kind) {
    case 'file_config_root':
      return [' (', ...roleParts(tag.data.role), ')']
    case 'extend_proxies_step':
      return [' → ', { profileId: tag.data.composition_id }]
    case 'scoped_transform':
      return [
        ' → ',
        { profileId: tag.data.host_profile_id },
        ' (',
        ...roleParts(tag.data.role),
        ')',
      ]
    default:
      return []
  }
}

export function stepParts(tag: OperatorTag): LabelPart[] {
  const kind = stepKind(tag)
  const subject = stepSubject(tag)
  return subject === undefined
    ? [kind]
    : [`${kind} · `, subject, ...stepContext(tag)]
}

/** A profile that no longer exists falls back to its raw id. */
export function profileName(profiles: ProfileLookup, id: string): string {
  return profiles.get(id)?.name ?? id
}

export function partsText(parts: LabelPart[], profiles: ProfileLookup) {
  return parts
    .map((part) =>
      typeof part === 'string' ? part : profileName(profiles, part.profileId),
    )
    .join('')
}

export function useProfileLookup(): ProfileLookup {
  const { query } = useProfile()
  return new Map(query.data?.items?.map((item) => [item.uid, item]) ?? [])
}

/** Opens the detail page of a profile; unknown ids are ignored. */
export function useOpenProfile(profiles: ProfileLookup) {
  const navigate = useNavigate()
  return (id: string) => {
    const profile = profiles.get(id)
    if (!profile) return
    navigate({
      to: '/main/profiles/$type/detail/$uid',
      params: { type: profileTypeOf(profile), uid: profile.uid },
    })
  }
}

export function StepLabel({
  parts,
  profiles,
  onOpenProfile,
}: {
  parts: LabelPart[]
  profiles: ProfileLookup
  onOpenProfile: (id: string) => void
}) {
  return parts.map((part, index) =>
    typeof part === 'string' ? (
      part
    ) : (
      <Tooltip key={index}>
        <TooltipTrigger asChild>
          <span
            className={profileTagClassName}
            onDoubleClick={() => onOpenProfile(part.profileId)}
          >
            {profileName(profiles, part.profileId)}
          </span>
        </TooltipTrigger>
        <TooltipContent className="font-mono">{part.profileId}</TooltipContent>
      </Tooltip>
    ),
  )
}

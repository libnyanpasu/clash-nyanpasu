import { m } from '@/paraglide/messages'
import type {
  ConfigExecutionRole,
  OperatorTag,
  ProfileItem_Serialize,
} from '@nyanpasu/rpc/types'

export type ProfileLabel = (id: string) => string

/** A step label: plain text runs and references to profiles by id. */
export type LabelPart = string | { profileId: string }

export type ProfileLookup = Map<string, ProfileItem_Serialize>

/** Preserve structured references through localized string interpolation. */
export function profileMessageParts(
  format: (label: ProfileLabel) => string,
): LabelPart[] {
  const ids: string[] = []
  const text = format((id) => `\0${ids.push(id) - 1}\0`)
  return text.split(/(\0\d+\0)/).map((part) => {
    const match = /^\0(\d+)\0$/.exec(part)
    return match && ids[Number(match[1])] !== undefined
      ? { profileId: ids[Number(match[1])]! }
      : part
  })
}

export function profileDialogLabel(
  profiles: ReadonlyMap<string, { name: string }>,
  id: string,
): string {
  const name = profiles.get(id)?.name
  return name ? `${name}（${id}）` : id
}

/** The selected profile needs no note, since its name is already shown. */
function roleParts(role: ConfigExecutionRole): LabelPart[] {
  switch (role.kind) {
    case 'selected':
      return []
    case 'composition_base':
      return [
        ' (',
        `${m.inspect_base()} → `,
        { profileId: role.data.composition_id },
        ')',
      ]
    case 'composition_contributor':
      return [
        ' (',
        `${m.inspect_contributor()} ${role.data.contributor_index + 1} → `,
        { profileId: role.data.composition_id },
        ')',
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
      return roleParts(tag.data.role)
    case 'extend_proxies_step':
      return [' → ', { profileId: tag.data.composition_id }]
    case 'scoped_transform':
      return [
        ' → ',
        { profileId: tag.data.host_profile_id },
        ...roleParts(tag.data.role),
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

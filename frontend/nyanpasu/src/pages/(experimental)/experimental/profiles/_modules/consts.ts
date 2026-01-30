import { m } from '@/paraglide/messages'
import { Profile, ScriptType } from '@nyanpasu/interface'

export enum ListType {
  Grid = 'grid',
  List = 'list',
}

export enum ProfileType {
  Profile = 'profile',
  JavaScript = 'javascript',
  Lua = 'lua',
  Merge = 'merge',
}

export const PROFILE_TYPE_NAMES = {
  [ProfileType.Profile]: m.profile_profile_label(),
  [ProfileType.JavaScript]: m.profile_javascript_label(),
  [ProfileType.Lua]: m.profile_lua_label(),
  [ProfileType.Merge]: m.profile_merge_label(),
} satisfies Record<ProfileType, string>

export const PROFILE_TYPES = {
  [ProfileType.Profile]: [{ type: 'remote' }, { type: 'local' }],
  [ProfileType.JavaScript]: [{ type: 'script', script_type: 'javascript' }],
  [ProfileType.Lua]: [{ type: 'script', script_type: 'lua' }],
  [ProfileType.Merge]: [{ type: 'merge' }],
} satisfies Record<
  ProfileType,
  Array<{
    type: Profile['type']
    script_type?: ScriptType
  }>
>

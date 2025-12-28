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

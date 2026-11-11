import type { NavigationListItemColor } from '@nyanpasu/ui/navigation-list'
import { m } from '@/paraglide/messages'

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

/** The tone of a type's round icon, shared by the navigation and the lists. */
export const PROFILE_TYPE_COLORS = {
  [ProfileType.Profile]: 'blue',
  [ProfileType.JavaScript]: 'yellow',
  [ProfileType.Lua]: 'cyan',
  [ProfileType.Merge]: 'orange',
} satisfies Record<ProfileType, NavigationListItemColor>

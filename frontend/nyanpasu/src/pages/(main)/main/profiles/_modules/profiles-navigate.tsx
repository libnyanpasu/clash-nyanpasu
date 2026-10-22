import DescriptionRounded from '~icons/material-symbols/description-rounded'
import DocumentSearchRounded from '~icons/material-symbols/document-search-rounded'
import JavascriptRounded from '~icons/material-symbols/javascript-rounded'
import MergeRounded from '~icons/material-symbols/merge-rounded'
import PublicIcon from '~icons/material-symbols/public'
import LuaIcon from '~icons/mdi/language-lua'
import { ComponentProps, ReactNode } from 'react'
import {
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/query'
import { Link, useMatchRoute } from '@tanstack/react-router'
import { categoryProfiles } from '../$type/_modules/utils'
import { ProfileType } from './consts'

function ProfilesNavigationItem({
  icon,
  color,
  label,
  description,
  ...props
}: ComponentProps<typeof Link> & {
  icon: ReactNode
  color: NavigationListItemColor
  label: string
  description: string
}) {
  const matchRoute = useMatchRoute()

  // An item stays selected on its child pages, such as a profile's detail page.
  const isActive = !!matchRoute({ to: props.to, fuzzy: true })

  return (
    <NavigationListItem active={isActive} asChild>
      <Link aria-current={isActive ? 'page' : undefined} {...props}>
        <NavigationListItemIcon color={color}>{icon}</NavigationListItemIcon>

        <NavigationListItemText headline={label} supporting={description} />
      </Link>
    </NavigationListItem>
  )
}

export default function ProfilesNavigate() {
  const {
    query: { data: profiles },
  } = useProfile()

  const categorized = categoryProfiles(profiles?.items)

  return (
    <NavigationList aria-label={m.profile_nav_label()}>
      <NavigationListGroup label={m.profile_group_config()}>
        <ProfilesNavigationItem
          to="/main/profiles/profile"
          color="blue"
          icon={<DescriptionRounded />}
          label={m.profile_nav_all_profiles()}
          description={m.profile_nav_count_profiles({
            count: categorized[ProfileType.Profile].length,
          })}
        />
      </NavigationListGroup>

      <NavigationListGroup label={m.profile_group_transform()}>
        <ProfilesNavigationItem
          to="/main/profiles/javascript"
          color="yellow"
          icon={<JavascriptRounded />}
          label={m.profile_nav_javascript()}
          description={m.profile_nav_count_scripts({
            count: categorized[ProfileType.JavaScript].length,
          })}
        />

        <ProfilesNavigationItem
          to="/main/profiles/lua"
          color="cyan"
          icon={<LuaIcon />}
          label={m.profile_nav_lua()}
          description={m.profile_nav_count_scripts({
            count: categorized[ProfileType.Lua].length,
          })}
        />

        <ProfilesNavigationItem
          to="/main/profiles/merge"
          color="orange"
          icon={<MergeRounded />}
          label={m.profile_nav_merge()}
          description={m.profile_nav_count_overlays({
            count: categorized[ProfileType.Merge].length,
          })}
        />
      </NavigationListGroup>

      {/* Both act on the active config's pipeline rather than on one profile. */}
      <NavigationListGroup label={m.profile_group_pipeline()}>
        <ProfilesNavigationItem
          to="/main/profiles/global"
          color="purple"
          icon={<PublicIcon />}
          label={m.profile_global_transform_title()}
          description={m.profile_nav_count_enabled({
            count: profiles?.global_transforms?.length ?? 0,
          })}
        />

        <ProfilesNavigationItem
          to="/main/profiles/inspect"
          color="green"
          icon={<DocumentSearchRounded />}
          label={m.inspect_title()}
          description={m.profile_nav_inspect_description()}
        />
      </NavigationListGroup>
    </NavigationList>
  )
}

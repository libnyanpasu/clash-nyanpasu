import AddRounded from '~icons/material-symbols/add-rounded'
import DescriptionRounded from '~icons/material-symbols/description-rounded'
import DocumentSearchRounded from '~icons/material-symbols/document-search-rounded'
import JavascriptRounded from '~icons/material-symbols/javascript-rounded'
import MergeRounded from '~icons/material-symbols/merge-rounded'
import PublicIcon from '~icons/material-symbols/public'
import LuaIcon from '~icons/mdi/language-lua'
import { Button } from '@nyanpasu/ui/button'
import {
  NavigationList,
  NavigationListGroup,
} from '@nyanpasu/ui/navigation-list'
import { m } from '@/paraglide/messages'
import { useDroppable } from '@dnd-kit/react'
import { useProfile } from '@nyanpasu/query'
import { ProfileType } from './consts'
import FolderNavItem from './folder-nav-item'
import ProfilesNavigationItem from './profiles-navigation-item'
import { NAV_ALL_ID, PROFILE_DRAG_TYPE } from './resolve-profile-drop'
import { useProfileLayout } from './use-profile-layout'
import { categoryProfiles } from './utils'

export default function ProfilesNavigate() {
  const {
    query: { data: profiles },
  } = useProfile()

  const { layout, openCreateFolder } = useProfileLayout()

  const categorized = categoryProfiles(profiles?.items)

  const folders = layout?.folders ?? []

  // A card dropped on All profiles leaves its folder.
  const { ref: allProfilesRef, isDropTarget: isAllProfilesDropTarget } =
    useDroppable({ id: NAV_ALL_ID, accept: PROFILE_DRAG_TYPE })

  return (
    <NavigationList aria-label={m.profile_nav_label()}>
      <NavigationListGroup
        label={m.profile_group_config()}
        action={
          <Button
            icon
            className="mr-2 size-8"
            aria-label={m.profile_folder_new_title()}
            data-slot="profiles-folder-add"
            onClick={() => openCreateFolder()}
          >
            <AddRounded className="size-5" />
          </Button>
        }
      >
        <ProfilesNavigationItem
          ref={allProfilesRef}
          to="/main/profiles/$type"
          params={{ type: ProfileType.Profile }}
          color="blue"
          icon={<DescriptionRounded />}
          label={m.profile_nav_all_profiles()}
          description={m.profile_nav_count_profiles({
            count: categorized[ProfileType.Profile].length,
          })}
          data-slot="profiles-all-nav-item"
          data-drop-target={isAllProfilesDropTarget || undefined}
        />

        {folders.map((folder, index) => (
          <FolderNavItem
            key={folder.id}
            folder={folder}
            index={index}
            count={folders.length}
          />
        ))}
      </NavigationListGroup>

      <NavigationListGroup label={m.profile_group_transform()}>
        <ProfilesNavigationItem
          to="/main/profiles/$type"
          params={{ type: ProfileType.JavaScript }}
          color="yellow"
          icon={<JavascriptRounded />}
          label={m.profile_nav_javascript()}
          description={m.profile_nav_count_scripts({
            count: categorized[ProfileType.JavaScript].length,
          })}
        />

        <ProfilesNavigationItem
          to="/main/profiles/$type"
          params={{ type: ProfileType.Lua }}
          color="cyan"
          icon={<LuaIcon />}
          label={m.profile_nav_lua()}
          description={m.profile_nav_count_scripts({
            count: categorized[ProfileType.Lua].length,
          })}
        />

        <ProfilesNavigationItem
          to="/main/profiles/$type"
          params={{ type: ProfileType.Merge }}
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

import AddRounded from '~icons/material-symbols/add-rounded'
import { useEffect } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  NavigationList,
  NavigationListGroup,
} from '@nyanpasu/ui/navigation-list'
import { m } from '@/paraglide/messages'
import { useClashProxies } from '@nyanpasu/query'
import { useParams } from '@tanstack/react-router'
import { useFolderCollapsed } from '../../_modules/use-folder-collapsed'
import GroupFolder from './group-folder'
import GroupListActions from './group-list-actions'
import GroupNavItem from './group-nav-item'
import { useGroupLayout } from './use-group-layout'

const COLLAPSED_KV_KEY = 'proxies-folder-collapsed'

export default function ProxiesNavigate() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const { layout, openCreateFolder } = useGroupLayout()

  const { isCollapsed, toggle, setAll } = useFolderCollapsed(COLLAPSED_KV_KEY)

  const { name: activeName } = useParams({ strict: false })

  const folderOfActive = layout?.folders.find((folder) =>
    activeName ? folder.members.includes(activeName) : false,
  )?.id

  // A selected group stays visible: opening it (from a link, the keyboard or
  // the tray) opens the folder it is in.
  useEffect(() => {
    if (folderOfActive) {
      setAll([folderOfActive], false)
    }
  }, [folderOfActive, activeName, setAll])

  if (!proxies || !layout) {
    return null
  }

  const groups = new Map(proxies.groups.map((group) => [group.name, group]))

  const renderGroup = (name: string) => {
    const group = groups.get(name)

    return group ? (
      <GroupNavItem
        key={name}
        group={group}
        proxies={proxies}
        active={name === activeName}
      />
    ) : null
  }

  const folderIds = layout.folders.map((folder) => folder.id)

  return (
    <NavigationList aria-label={m.proxies_nav_label()}>
      <NavigationListGroup
        label={m.proxies_nav_groups()}
        action={
          <GroupListActions
            hasFolders={folderIds.length > 0}
            onExpandAll={() => setAll(folderIds, false)}
            onCollapseAll={() => setAll(folderIds, true)}
          />
        }
      >
        {layout.ungrouped.map(renderGroup)}
      </NavigationListGroup>

      <NavigationListGroup
        label={m.proxies_nav_folders()}
        className="[&>[role=group]]:gap-2"
        action={
          <Button
            icon
            className="mr-2 size-8"
            aria-label={m.profile_folder_new_title()}
            data-slot="proxies-folder-add"
            onClick={() => openCreateFolder()}
          >
            <AddRounded className="size-5" />
          </Button>
        }
      >
        {layout.folders.map((folder, index) => (
          <GroupFolder
            key={folder.id}
            folder={folder}
            index={index}
            count={layout.folders.length}
            expanded={!isCollapsed(folder.id)}
            onToggle={() => toggle(folder.id)}
          >
            {folder.members.map(renderGroup)}
          </GroupFolder>
        ))}
      </NavigationListGroup>
    </NavigationList>
  )
}

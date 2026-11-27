import FolderOpenRounded from '~icons/material-symbols/folder-open-rounded'
import {
  MenubarContent,
  MenubarMenu,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../main/profiles/_modules/consts'
import { Action } from '../main/profiles/$type/index'
import { useHeaderMenuPrimitives } from './header-menu-primitives'

export function HeaderFileItems() {
  const { mode, Item } = useHeaderMenuPrimitives()

  return (
    <Item asChild>
      <Link
        to="/main/profiles/$type"
        params={{
          type: ProfileType.Profile,
        }}
        search={{
          action: Action.ImportLocalProfile,
        }}
      >
        {mode === 'launcher' && (
          <FolderOpenRounded className="size-5 shrink-0" aria-hidden="true" />
        )}
        <span>{m.header_file_action_import_local_profile()}</span>
      </Link>
    </Item>
  )
}

export default function HeaderFileAction() {
  return (
    <MenubarMenu value="file">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_file_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <HeaderFileItems />
      </MenubarContent>
    </MenubarMenu>
  )
}

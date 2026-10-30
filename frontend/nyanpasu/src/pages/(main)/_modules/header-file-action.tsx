import {
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../main/profiles/_modules/consts'
import { Action } from '../main/profiles/$type/index'

export default function HeaderFileAction() {
  return (
    <MenubarMenu value="file">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_file_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <MenubarItem asChild>
          <Link
            to="/main/profiles/$type"
            params={{
              type: ProfileType.Profile,
            }}
            search={{
              action: Action.ImportLocalProfile,
            }}
          >
            {m.header_file_action_import_local_profile()}
          </Link>
        </MenubarItem>
      </MenubarContent>
    </MenubarMenu>
  )
}

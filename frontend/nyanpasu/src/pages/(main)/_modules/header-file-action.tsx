import { PropsWithChildren } from 'react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../main/profiles/_modules/consts'
import { Action } from '../main/profiles/$type/index'

export default function HeaderFileAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>

      <DropdownMenuContent>
        <DropdownMenuItem asChild>
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
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

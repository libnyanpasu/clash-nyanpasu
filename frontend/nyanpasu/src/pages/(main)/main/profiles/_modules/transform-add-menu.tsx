import AddRounded from '~icons/material-symbols/add-rounded'
import { Fragment } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { PROFILE_TYPE_NAMES, ProfileType } from './consts'
import { categoryProfiles, type TransformProfile } from './utils'

const TRANSFORM_TYPES = [
  ProfileType.JavaScript,
  ProfileType.Lua,
  ProfileType.Merge,
] as const

/** Lists the transforms that are not in the chain yet, grouped by type. */
export function TransformAddMenu({
  available,
  disabled,
  onAdd,
}: {
  available: TransformProfile[]
  disabled?: boolean
  onAdd: (uid: string) => void
}) {
  const categorized = categoryProfiles(available)
  const groups = TRANSFORM_TYPES.filter((type) => categorized[type].length > 0)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="stroked"
          data-slot="profile-chain-editor-add"
          disabled={disabled}
          className="flex items-center gap-2"
        >
          <AddRounded className="size-5" aria-hidden />
          {m.profile_chain_editor_add()}
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="end" className="max-h-80 overflow-y-auto">
        {groups.length === 0 ? (
          <DropdownMenuItem disabled>
            {m.profile_chain_editor_add_empty()}
          </DropdownMenuItem>
        ) : (
          groups.map((type, index) => (
            <Fragment key={type}>
              {index > 0 && <DropdownMenuSeparator />}

              <DropdownMenuGroup>
                <DropdownMenuLabel className="text-primary h-9 text-xs font-medium">
                  {PROFILE_TYPE_NAMES[type]}
                </DropdownMenuLabel>

                {categorized[type].map((profile) => (
                  <DropdownMenuItem
                    key={profile.uid}
                    data-uid={profile.uid}
                    onSelect={() => onAdd(profile.uid)}
                  >
                    {profile.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            </Fragment>
          ))
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

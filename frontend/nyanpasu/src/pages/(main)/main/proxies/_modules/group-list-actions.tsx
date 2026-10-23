import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import UnfoldLessRounded from '~icons/material-symbols/unfold-less-rounded'
import UnfoldMoreRounded from '~icons/material-symbols/unfold-more-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useGroupLayout } from './use-group-layout'

/**
 * The overflow menu of the group list: reset the order, or open and close
 * every folder.
 */
export default function GroupListActions({
  hasFolders,
  onExpandAll,
  onCollapseAll,
}: {
  hasFolders: boolean
  onExpandAll: () => void
  onCollapseAll: () => void
}) {
  const { arranged, resetOrder } = useGroupLayout()

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          icon
          className="mr-2 size-8"
          aria-label={m.proxies_nav_list_actions()}
          data-slot="proxies-list-actions"
        >
          <MoreVertRounded className="size-5" aria-hidden />
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="end">
        <DropdownMenuItem disabled={!arranged} onSelect={() => resetOrder()}>
          <RestartAltRounded className="size-4" aria-hidden />
          <span>{m.proxies_nav_reset_order()}</span>
        </DropdownMenuItem>

        <DropdownMenuSeparator />

        <DropdownMenuItem disabled={!hasFolders} onSelect={onExpandAll}>
          <UnfoldMoreRounded className="size-4" aria-hidden />
          <span>{m.proxies_nav_expand_all()}</span>
        </DropdownMenuItem>

        <DropdownMenuItem disabled={!hasFolders} onSelect={onCollapseAll}>
          <UnfoldLessRounded className="size-4" aria-hidden />
          <span>{m.proxies_nav_collapse_all()}</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

import Radar from '~icons/material-symbols/radar'
import SortRounded from '~icons/material-symbols/sort-rounded'
import VisibilityOffRounded from '~icons/material-symbols/visibility-off-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { SearchField } from '@nyanpasu/ui/search-field'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { NODE_SORTS, type NodeSort, type NodeView } from './node-list'

const SORT_LABELS = {
  default: m.proxies_node_sort_default,
  name: m.proxies_node_sort_name,
  delay: m.proxies_node_sort_delay,
} satisfies Record<NodeSort, () => string>

export function NodeListToolbar({
  search,
  onSearchChange,
}: {
  search: string
  onSearchChange: (search: string) => void
}) {
  return (
    <div
      className="flex flex-wrap items-center gap-2 pb-2"
      data-slot="proxies-node-list-toolbar"
    >
      {/* Too narrow to read a term, the controls wrap instead of squeezing it. */}
      <SearchField
        className="min-w-40 flex-1"
        placeholder={m.proxies_node_search_placeholder()}
        clearLabel={m.proxies_node_search_clear()}
        value={search}
        onValueChange={onSearchChange}
      />
    </div>
  )
}

export function SortNodesButton({
  view,
  onViewChange,
}: {
  view: NodeView
  onViewChange: (view: NodeView) => void
}) {
  return (
    <DropdownMenu align="end">
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <Button
              icon
              className="size-8 shrink-0"
              aria-label={m.proxies_node_sort_label()}
              data-slot="proxies-node-sort-button"
            >
              <SortRounded className="size-4" />
            </Button>
          </DropdownMenuTrigger>
        </TooltipTrigger>

        <TooltipContent>{m.proxies_node_sort_label()}</TooltipContent>
      </Tooltip>

      <DropdownMenuContent>
        <DropdownMenuRadioGroup
          value={view.sort}
          onValueChange={(sort) =>
            onViewChange({ ...view, sort: sort as NodeSort })
          }
        >
          {NODE_SORTS.map((sort) => (
            <DropdownMenuRadioItem key={sort} value={sort}>
              {SORT_LABELS[sort]()}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function HideUnavailableButton({
  pressed,
  onPressedChange,
}: {
  pressed: boolean
  onPressedChange: (pressed: boolean) => void
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          icon
          className="aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container size-8 shrink-0"
          aria-label={m.proxies_node_hide_unavailable()}
          aria-pressed={pressed}
          onClick={() => onPressedChange(!pressed)}
          data-slot="proxies-node-hide-unavailable"
        >
          <VisibilityOffRounded className="size-4" />
        </Button>
      </TooltipTrigger>

      <TooltipContent>{m.proxies_node_hide_unavailable()}</TooltipContent>
    </Tooltip>
  )
}

export function LocateCurrentNodeButton({
  disabled,
  onLocate,
}: {
  disabled: boolean
  onLocate: () => void
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          icon
          className="size-8 shrink-0"
          disabled={disabled}
          aria-label={m.proxies_locate_current_node()}
          onClick={onLocate}
          data-slot="proxies-locate-current-node"
        >
          <Radar className="size-4" />
        </Button>
      </TooltipTrigger>

      <TooltipContent>{m.proxies_locate_current_node()}</TooltipContent>
    </Tooltip>
  )
}

export function NoMatchingNodes() {
  return (
    <p
      data-slot="proxies-node-no-match"
      className="text-on-surface-variant p-8 text-center text-sm"
    >
      {m.proxies_node_no_match()}
    </p>
  )
}

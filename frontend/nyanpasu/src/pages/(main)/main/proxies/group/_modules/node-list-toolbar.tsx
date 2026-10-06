import Radar from '~icons/material-symbols/radar'
import SearchRounded from '~icons/material-symbols/search-rounded'
import SortRounded from '~icons/material-symbols/sort-rounded'
import VisibilityOffRounded from '~icons/material-symbols/visibility-off-rounded'
import {
  AnimatePresence,
  motion,
  useReducedMotion,
  type Transition,
} from 'motion/react'
import { useState } from 'react'
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

export function SearchNodesButton({
  search,
  onSearchChange,
}: {
  search: string
  onSearchChange: (search: string) => void
}) {
  const [open, setOpen] = useState(false)

  const reduceMotion = useReducedMotion()

  const transition: Transition = reduceMotion
    ? { duration: 0 }
    : { duration: 0.2, ease: 'easeOut' }

  return (
    <AnimatePresence initial={false} mode="wait">
      {open ? (
        <motion.div
          key="field"
          className="h-8 shrink-0"
          initial={{ width: 0, opacity: 0 }}
          animate={{ width: '12rem', opacity: 1 }}
          exit={{ width: 0, opacity: 0 }}
          transition={transition}
        >
          <SearchField
            className="h-8 w-full overflow-hidden"
            autoFocus
            placeholder={m.proxies_node_search_placeholder()}
            clearLabel={m.proxies_node_search_clear()}
            value={search}
            onValueChange={onSearchChange}
            onKeyDown={(event) => {
              if (event.key === 'Escape' && !search) {
                setOpen(false)
              }
            }}
            onBlur={() => {
              if (!search) {
                setOpen(false)
              }
            }}
            data-slot="proxies-node-search-field"
          />
        </motion.div>
      ) : (
        <motion.div
          key="button"
          className="shrink-0"
          initial={{ opacity: 0, scale: 0.8 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={transition}
        >
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                icon
                className="size-8 shrink-0"
                aria-label={m.proxies_node_search_placeholder()}
                onClick={() => setOpen(true)}
                data-slot="proxies-node-search-button"
              >
                <SearchRounded className="size-4" />
              </Button>
            </TooltipTrigger>

            <TooltipContent>
              {m.proxies_node_search_placeholder()}
            </TooltipContent>
          </Tooltip>
        </motion.div>
      )}
    </AnimatePresence>
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

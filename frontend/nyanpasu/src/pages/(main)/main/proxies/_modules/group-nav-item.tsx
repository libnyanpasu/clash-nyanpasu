import AltRouteRounded from '~icons/material-symbols/alt-route-rounded'
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import BalanceRounded from '~icons/material-symbols/balance-rounded'
import KeepRounded from '~icons/material-symbols/keep-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import SpeedRounded from '~icons/material-symbols/speed-rounded'
import TouchAppRounded from '~icons/material-symbols/touch-app-rounded'
import { memo, type ComponentType, type SVGProps } from 'react'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  NavigationListItemTrailing,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import DelayChip from '@/components/proxies/delay-chip'
import { CacheImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { FOLDER_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { pointerIntersection } from '@dnd-kit/collision'
import { useSortable } from '@dnd-kit/react/sortable'
import {
  memberDelay,
  useSetting,
  type ClashProxiesQuery,
  type ClashProxiesQueryGroupItem,
} from '@nyanpasu/query'
import type { FolderId } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { locateGroup } from './group-layout'
import { GroupMoveContextMenu, useGroupMove } from './group-move-menu'
import { GROUP_DRAG_TYPE, groupDragId, sectionId } from './resolve-group-drop'
import { useGroupLayout } from './use-group-layout'

type GroupGlyph = {
  color: NavigationListItemColor
  icon: ComponentType<SVGProps<SVGSVGElement>>
}

/** What stands in for the icon of a group that has none. */
const GLYPHS: Record<string, GroupGlyph> = {
  Selector: { color: 'blue', icon: TouchAppRounded },
  URLTest: { color: 'green', icon: SpeedRounded },
  Fallback: { color: 'orange', icon: ShieldRounded },
  LoadBalance: { color: 'purple', icon: BalanceRounded },
  Relay: { color: 'cyan', icon: RouteRounded },
}

const FALLBACK_GLYPH: GroupGlyph = { color: 'neutral', icon: AltRouteRounded }

function GroupDelay({
  group,
  proxies,
}: {
  group: ClashProxiesQueryGroupItem
  proxies: ClashProxiesQuery
}) {
  const { value: defaultUrl } = useSetting('default_latency_test')

  const delay = group.now
    ? memberDelay(group.now, group, proxies, defaultUrl ?? '')
    : undefined

  if (delay === undefined) {
    return null
  }

  return (
    <span className="text-xs tabular-nums">
      {delay <= 0 ? (
        <span className="text-red-500" title={m.proxies_delay_history_failed()}>
          ∞
        </span>
      ) : (
        <DelayChip delay={delay} />
      )}
    </span>
  )
}

/**
 * A group in the sidebar: a link to its page, with the type, pin, current node
 * and delay it showed before, and a menu to arrange it. Memoized so that
 * switching groups re-renders only the items whose active state changed.
 */
const GroupNavItem = memo(function GroupNavItem({
  group,
  proxies,
  active,
  index,
  section,
}: {
  group: ClashProxiesQueryGroupItem
  proxies: ClashProxiesQuery
  active: boolean
  /** Where the group sits in its list, as the list shows it now. */
  index: number
  /** The folder the group is in; `null` for the ungrouped groups. */
  section: FolderId | null
}) {
  const { layout, moveGroupBy } = useGroupLayout()
  const move = useGroupMove(group.name)

  const place = layout ? locateGroup(layout, group.name) : undefined

  const glyph = GLYPHS[group.type] ?? FALLBACK_GLYPH

  // The whole item is the drag source; a click still opens the group, so a
  // mouse drag starts only after the pointer moved.
  const { ref, isDragging } = useSortable({
    id: groupDragId(group.name),
    index,
    group: sectionId(section),
    type: GROUP_DRAG_TYPE,
    accept: [GROUP_DRAG_TYPE],
    collisionDetector: pointerIntersection,
    sensors: FOLDER_ITEM_DRAG_SENSORS,
  })

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <NavigationListItem
          ref={ref}
          active={active}
          asChild
          data-dragging={isDragging || undefined}
        >
          <Link
            // dnd-kit gives its drag source the button role unless one is set.
            role="link"
            aria-current={active ? 'page' : undefined}
            to="/main/proxies/group/$name"
            params={{ name: group.name }}
            search={(previous) => ({ q: previous.q })}
          >
            {group.icon ? (
              <NavigationListItemIcon image>
                <CacheImage
                  icon={group.icon}
                  className="size-full"
                  // The picture is never clipped, but its placeholder stays round.
                  loadingClassName="rounded-full"
                />
              </NavigationListItemIcon>
            ) : (
              <NavigationListItemIcon color={glyph.color}>
                <glyph.icon />
              </NavigationListItemIcon>
            )}

            <NavigationListItemText
              headline={group.name}
              supporting={group.now ?? ''}
              supportingStart={
                <>
                  <span className="bg-secondary-container text-on-secondary-container shrink-0 rounded-full px-1.5 py-0.5 text-[10px] leading-3">
                    {group.type}
                  </span>

                  {group.now && group.fixed && (
                    <span
                      className="shrink-0"
                      title={group.fixed}
                      data-slot="group-summary-fixed-icon"
                    >
                      <KeepRounded className="size-3.5" />
                    </span>
                  )}
                </>
              }
            />

            <NavigationListItemTrailing>
              <GroupDelay group={group} proxies={proxies} />
            </NavigationListItemTrailing>
          </Link>
        </NavigationListItem>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <GroupMoveContextMenu move={move} />

        <ContextMenuItem
          disabled={!place || place.index === 0}
          onClick={() => moveGroupBy(group.name, -1)}
        >
          <ArrowUpwardRounded className="size-4" />
          <span>{m.profile_folder_move_up()}</span>
        </ContextMenuItem>

        <ContextMenuItem
          disabled={!place || place.index === place.count - 1}
          onClick={() => moveGroupBy(group.name, 1)}
        >
          <ArrowDownwardRounded className="size-4" />
          <span>{m.profile_folder_move_down()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
})

export default GroupNavItem

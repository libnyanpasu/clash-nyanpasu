import KeepOffRounded from '~icons/material-symbols/keep-off-rounded'
import KeepRounded from '~icons/material-symbols/keep-rounded'
import {
  AnimatePresence,
  motion,
  useReducedMotion,
  type Transition,
} from 'motion/react'
import { useCallback, useDeferredValue, useMemo, useRef, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { useContainerBreakpointValue, useLockFn } from '@nyanpasu/hooks'
import {
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
  groupTestUrl,
  memberState,
  resolveChain,
  useClashProxies,
  useKvStorage,
  useProxyMode,
  useSetting,
  type MemberState,
} from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useSearchTerm } from '../../_modules/use-search-term'
import DelayTestButton from './_modules/delay-test-button'
import GroupHeader from './_modules/group-header'
import {
  GroupMeta,
  GroupStatus,
  GroupTrafficSpeed,
} from './_modules/group-meta'
import {
  DEFAULT_NODE_VIEW,
  NODE_VIEW_KV_KEY,
  toNodeView,
  visibleMembers,
  type NodeView,
} from './_modules/node-list'
import {
  HideUnavailableButton,
  LocateCurrentNodeButton,
  NodeSearchOverlay,
  NoMatchingNodes,
  SearchNodesButton,
  SortNodesButton,
} from './_modules/node-list-toolbar'
import ProxyNodeButton from './_modules/proxy-node-button'

export const Route = createFileRoute('/(main)/main/proxies/group/$name')({
  component: RouteComponent,
})

// Every node card is as tall as the fab button's fixed h-14 plus the item's
// p-1. A fixed size spares the virtualizer measuring each mounted card, which
// forced a layout and a second render every time a group opened.
const NODE_ITEM_HEIGHT = 64

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams()

  const { q } = Route.useSearch()

  const navigate = Route.useNavigate()

  const writeQuery = useCallback(
    (next: string | undefined) =>
      navigate({
        search: (previous) => ({ ...previous, q: next }),
        replace: true,
      }),
    [navigate],
  )

  const [search, setSearch] = useSearchTerm(q, writeQuery)

  // Filtering and highlighting the nodes follows the field without holding
  // up typing.
  const deferredSearch = useDeferredValue(search)

  const [storedView, setView] = useKvStorage<NodeView>(
    NODE_VIEW_KV_KEY,
    DEFAULT_NODE_VIEW,
    { migrate: toNodeView },
  )

  const handleViewChange = useCallback(
    async (next: NodeView) => {
      if (!(await setView(next))) {
        message(m.proxies_node_view_save_failed(), { kind: 'error' })
      }
    },
    [setView],
  )

  const {
    proxies: { data: proxies },
    selectProxy,
    clearProxyFixed,
    updateProxiesDelay: { mutateAsync: mutateProxyDelay },
  } = useClashProxies()

  const { value: proxyMode } = useProxyMode()

  const currentGroup = useMemo<ClashProxiesQueryGroupItem | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global ?? undefined
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName)
  }, [proxies, proxyGroupName, proxyMode])

  const groupName = currentGroup?.name

  const { value: defaultUrl } = useSetting('default_latency_test')

  const testUrl = groupTestUrl(currentGroup, defaultUrl ?? '')

  const delayOptions = useMemo(
    () => ({ url: testUrl, expected: currentGroup?.expectedStatus ?? null }),
    [testUrl, currentGroup?.expectedStatus],
  )

  // Each member's delay, history and, for a nested group, the node it
  // resolves to, computed once per snapshot rather than in every card.
  const memberStates = useMemo(() => {
    const states = new Map<string, MemberState>()

    if (!currentGroup || !proxies) {
      return states
    }

    for (const name of currentGroup.all) {
      states.set(
        name,
        memberState(name, currentGroup, proxies, defaultUrl ?? ''),
      )
    }

    return states
  }, [currentGroup, proxies, defaultUrl])

  const groupStatus = useMemo(() => {
    if (!currentGroup || !proxies) {
      return undefined
    }

    let available = 0

    for (const { delay } of memberStates.values()) {
      if ((delay ?? 0) > 0) {
        available += 1
      }
    }

    return {
      available,
      total: currentGroup.all.length,
      chain: resolveChain(currentGroup.name, proxies).path,
    }
  }, [currentGroup, proxies, memberStates])

  const members = useMemo(
    () =>
      currentGroup && proxies
        ? visibleMembers({
            group: currentGroup,
            proxies,
            query: deferredSearch,
            view: storedView,
            delayOf: (name) => memberStates.get(name)?.delay,
          })
        : [],
    [currentGroup, proxies, deferredSearch, storedView, memberStates],
  )

  // HighlightText marks one substring, so a query of several terms marks
  // nothing.
  const trimmedSearch = deferredSearch.trim()

  const searchText = trimmedSearch.includes(' ') ? '' : trimmedSearch

  const selectable = currentGroup?.capabilities.select ?? false

  const handleSelectProxy = useCallback(
    async (proxy: ClashProxiesQueryProxyItem) => {
      if (!groupName) {
        return
      }

      try {
        await selectProxy(groupName, proxy.name)
      } catch (error) {
        message(
          m.proxies_select_failed_message({
            group: groupName,
            name: proxy.name,
          }),
          { kind: 'error', error },
        )
      }
    },
    [groupName, selectProxy],
  )

  const handleClearFixed = useLockFn(async () => {
    if (!groupName) {
      return
    }

    try {
      await clearProxyFixed(groupName)
    } catch (error) {
      message(m.proxies_clear_fixed_failed_message({ group: groupName }), {
        kind: 'error',
        error,
      })
    }
  })

  const handleDelayTest = useCallback(
    async (proxy: ClashProxiesQueryProxyItem) => {
      await mutateProxyDelay([proxy.name, proxy.provider, delayOptions])
    },
    [mutateProxyDelay, delayOptions],
  )

  const { viewportRef } = useScrollAreaViewport()

  const [searchOpen, setSearchOpen] = useState(false)

  const headerRef = useRef<HTMLDivElement>(null)

  // Where the search icon sits across the header row, as a 0..1 fraction, so
  // the narrow overlay can unfold from that point out to both sides.
  const [searchOrigin, setSearchOrigin] = useState(0.5)

  // The overlay starts after the back button so it never covers it.
  const [searchOverlayLeft, setSearchOverlayLeft] = useState(0)

  const reduceMotion = useReducedMotion()

  const headerFade: Transition = reduceMotion
    ? { duration: 0 }
    : { duration: 0.2, ease: 'easeOut' }

  // Collapsed to a hairline at the search icon, the overlay unfolds both ways.
  const searchOverlayClip = `inset(0% ${(1 - searchOrigin) * 100}% 0% ${
    searchOrigin * 100
  }%)`

  // Below this width the header gives the whole row to an opened search
  // instead of squeezing the title, its status and the actions.
  const narrowHeader = useContainerBreakpointValue(
    viewportRef,
    { xs: true, sm: false },
    false,
  )

  const handleSearchOpenChange = useCallback((open: boolean) => {
    setSearchOpen(open)

    if (!open) {
      return
    }

    const header = headerRef.current
    const button = header?.querySelector(
      '[data-slot="proxies-node-search-button"]',
    )
    const row = header?.querySelector('[data-slot="proxies-group-header-row"]')

    if (!button || !row) {
      return
    }

    const rowRect = row.getBoundingClientRect()

    if (rowRect.width === 0) {
      return
    }

    const buttonRect = button.getBoundingClientRect()
    const back = header?.querySelector(
      '[data-slot="proxies-group-back-button"]',
    )
    const overlayLeft = back
      ? back.getBoundingClientRect().right - rowRect.left
      : 0
    const overlayWidth = rowRect.width - overlayLeft
    const center = buttonRect.left + buttonRect.width / 2 - rowRect.left

    setSearchOverlayLeft(overlayLeft)

    setSearchOrigin(
      overlayWidth > 0 ? Math.min(Math.max(center / overlayWidth, 0), 1) : 0.5,
    )
  }, [])

  // define the number of lanes based on the container breakpoint
  const lanes = useContainerBreakpointValue(
    viewportRef,
    {
      xs: 2,
      sm: 3,
      md: 4,
      lg: 5,
      xl: 6,
    },
    4,
  )

  const virtualizer = useVirtualizer({
    count: members.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => NODE_ITEM_HEIGHT,
    overscan: 5,
    lanes,
  })

  const virtualItems = virtualizer.getVirtualItems()

  // Mounting the node cards is the bulk of opening a group. Router updates
  // render synchronously, so the cards mount in a deferred render instead:
  // the page commits and starts its transition at once, and React renders
  // the cards in interruptible chunks right after.
  const showNodes = useDeferredValue(true, false)

  // -1 while the current node is filtered out or the group has none.
  const currentIndex = currentGroup?.now
    ? members.indexOf(currentGroup.now)
    : -1

  const handleScrollToCurrentNode = useCallback(() => {
    virtualizer.scrollToIndex(currentIndex, {
      align: 'center',
      behavior: 'smooth',
    })
  }, [currentIndex, virtualizer])

  const noMatch = members.length === 0 && Boolean(currentGroup?.all.length)

  return (
    <>
      <GroupHeader ref={headerRef}>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          {currentGroup && groupStatus ? (
            <GroupMeta
              name={currentGroup.name}
              status={
                <GroupStatus
                  available={groupStatus.available}
                  total={groupStatus.total}
                  chain={groupStatus.chain}
                />
              }
              speed={<GroupTrafficSpeed groupName={currentGroup.name} />}
              speedCompact={
                <GroupTrafficSpeed
                  groupName={currentGroup.name}
                  only="fastest"
                />
              }
            />
          ) : (
            <span className="min-w-0 truncate font-medium">
              {currentGroup?.name}
            </span>
          )}

          {currentGroup?.fixed && (
            <div
              className="text-on-surface-variant flex min-w-0 items-center gap-1 text-xs"
              title={currentGroup.fixed}
              data-slot="proxies-group-fixed"
            >
              <KeepRounded className="size-3.5 shrink-0" />
              <span className="shrink-0">{m.proxies_group_fixed_label()}</span>
              <span className="truncate">{currentGroup.fixed}</span>
            </div>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <SearchNodesButton
            open={narrowHeader ? false : searchOpen}
            onOpenChange={handleSearchOpenChange}
            search={search}
            onSearchChange={setSearch}
          />

          {currentGroup?.fixed && currentGroup.capabilities.clearFixed && (
            <Button
              variant="stroked"
              className="flex h-8 shrink-0 items-center gap-1 px-3 text-sm"
              onClick={handleClearFixed}
              data-slot="proxies-group-clear-fixed-button"
            >
              <KeepOffRounded className="size-4" />
              <span>{m.proxies_group_clear_fixed_button()}</span>
            </Button>
          )}

          <SortNodesButton view={storedView} onViewChange={handleViewChange} />

          <HideUnavailableButton
            pressed={storedView.hideUnavailable}
            onPressedChange={(hideUnavailable) =>
              handleViewChange({ ...storedView, hideUnavailable })
            }
          />

          <LocateCurrentNodeButton
            disabled={currentIndex === -1}
            onLocate={handleScrollToCurrentNode}
          />
        </div>

        <AnimatePresence initial={false}>
          {narrowHeader && searchOpen && (
            <motion.div
              key="proxies-search-overlay"
              className="bg-mixed-background absolute inset-y-0 right-0 z-20 flex items-center px-1"
              data-slot="proxies-node-search-overlay"
              style={{ left: searchOverlayLeft }}
              initial={{ clipPath: searchOverlayClip, opacity: 0.4 }}
              animate={{ clipPath: 'inset(0% 0% 0% 0%)', opacity: 1 }}
              exit={{ clipPath: searchOverlayClip, opacity: 0.4 }}
              transition={headerFade}
            >
              <NodeSearchOverlay
                search={search}
                onSearchChange={setSearch}
                onClose={() => setSearchOpen(false)}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </GroupHeader>

      {noMatch ? (
        <NoMatchingNodes />
      ) : (
        <div
          className="relative m-2"
          data-slot="proxies-virtual-list"
          style={{
            width: 'calc(100% - 16px)',
            height: `${virtualizer.getTotalSize()}px`,
          }}
        >
          {showNodes &&
            virtualItems.map((virtualItem) => {
              const name = members[virtualItem.index]
              const proxy = name ? proxies?.nodes[name] : undefined
              const state = name ? memberStates.get(name) : undefined

              if (!proxy || !state) {
                return null
              }

              return (
                <div
                  key={name}
                  className="group absolute top-0 left-0 p-1"
                  style={{
                    height: `${virtualItem.size}px`,
                    transform: `translateY(${virtualItem.start}px)`,
                    width: `${100 / lanes}%`,
                    left: `${virtualItem.lane * (100 / lanes)}%`,
                  }}
                  data-index={virtualItem.index}
                  data-slot="proxies-virtual-item"
                  data-active={String(name === currentGroup?.now)}
                >
                  <ProxyNodeButton
                    proxy={proxy}
                    selectable={selectable}
                    fixed={name === currentGroup?.fixed}
                    onSelect={handleSelectProxy}
                    onDelayTest={handleDelayTest}
                    history={state.history}
                    delay={state.delay}
                    leaf={state.leaf}
                    searchText={searchText}
                  />
                </div>
              )
            })}
        </div>
      )}

      <DelayTestButton delayOptions={delayOptions} />
    </>
  )
}

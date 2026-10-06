import ArrowDownwardAltRounded from '~icons/material-symbols/arrow-downward-alt-rounded'
import ArrowUpwardAltRounded from '~icons/material-symbols/arrow-upward-alt-rounded'
import KeepOffRounded from '~icons/material-symbols/keep-off-rounded'
import KeepRounded from '~icons/material-symbols/keep-rounded'
import { filesize } from 'filesize'
import { useCallback, useDeferredValue, useMemo } from 'react'
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
import { useGroupTrafficSpeed } from '../_modules/hooks'
import { useSearchTerm } from '../../_modules/use-search-term'
import DelayTestButton from './_modules/delay-test-button'
import GroupHeader from './_modules/group-header'
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
  NodeListToolbar,
  NoMatchingNodes,
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

// Subscribes to connection samples on its own so each sample re-renders only
// this label, not the node grid.
function GroupTrafficSpeed({ groupName }: { groupName?: string }) {
  const speed = useGroupTrafficSpeed(groupName)

  return (
    <>
      <div className="flex items-center">
        <ArrowDownwardAltRounded className="size-6" />

        <span className="text-sm">
          {filesize(speed.download, {
            standard: 'iec',
          })}
          /s
        </span>
      </div>

      <div className="flex items-center">
        <ArrowUpwardAltRounded className="size-6" />

        <span className="text-sm">
          {filesize(speed.upload, {
            standard: 'iec',
          })}
          /s
        </span>
      </div>
    </>
  )
}

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
      <GroupHeader
        bottom={<NodeListToolbar search={search} onSearchChange={setSearch} />}
      >
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <div className="flex max-w-full min-w-0 flex-col gap-1">
            <div className="truncate" title={currentGroup?.name}>
              {currentGroup?.name}
            </div>

            {groupStatus && (
              <div
                className="text-on-surface-variant flex min-w-0 items-center gap-1 text-xs"
                data-slot="proxies-group-status"
              >
                <span className="shrink-0">
                  {m.proxies_group_available({
                    available: groupStatus.available,
                    total: groupStatus.total,
                  })}
                </span>

                {groupStatus.chain.length > 0 && (
                  <>
                    <span className="shrink-0">·</span>
                    <span
                      className="truncate"
                      title={groupStatus.chain.join(' › ')}
                    >
                      {groupStatus.chain.join(' › ')}
                    </span>
                  </>
                )}
              </div>
            )}

            {currentGroup?.fixed && (
              <div
                className="text-on-surface-variant flex min-w-0 items-center gap-1 text-xs"
                title={currentGroup.fixed}
                data-slot="proxies-group-fixed"
              >
                <KeepRounded className="size-3.5 shrink-0" />
                <span className="shrink-0">
                  {m.proxies_group_fixed_label()}
                </span>
                <span className="truncate">{currentGroup.fixed}</span>
              </div>
            )}
          </div>

          <GroupTrafficSpeed groupName={currentGroup?.name} />
        </div>

        <div className="flex-1" />

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

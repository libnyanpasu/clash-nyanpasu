import AddRounded from '~icons/material-symbols/add-rounded'
import AppsRounded from '~icons/material-symbols/apps-rounded'
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import CloudSyncRounded from '~icons/material-symbols/cloud-sync-rounded'
import DataUsageRounded from '~icons/material-symbols/data-usage-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import DoneRounded from '~icons/material-symbols/done-rounded'
import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import NetworkCheckRounded from '~icons/material-symbols/network-check-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import RuleFolderRounded from '~icons/material-symbols/rule-folder-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { cva } from 'class-variance-authority'
import { useState, type ComponentType } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { DndGridSource } from '@nyanpasu/ui/dnd-grid'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerHandle,
  DrawerTitle,
} from '@nyanpasu/ui/drawer'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SearchField } from '@nyanpasu/ui/search-field'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import {
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WIDGET_RECOMMENDED_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts'
import { useDashboardContext } from '@/components/widgets/provider'
import { m } from '@/paraglide/messages'

const WIDGET_TITLE_MAP: Record<WidgetId, () => string> = {
  [WidgetId.TrafficDown]: m.dashboard_widget_traffic_download,
  [WidgetId.TrafficUp]: m.dashboard_widget_traffic_upload,
  [WidgetId.Connections]: m.dashboard_widget_connections,
  [WidgetId.Memory]: m.dashboard_widget_memory,
  [WidgetId.ProxyShortcuts]: m.dashboard_widget_proxy_status,
  [WidgetId.CoreShortcuts]: m.dashboard_widget_core_status,
  [WidgetId.SubscriptionQuota]: m.dashboard_widget_subscription_quota_title,
  [WidgetId.SubscriptionSchedule]:
    m.dashboard_widget_subscription_schedule_title,
  [WidgetId.ProxyMode]: m.dashboard_widget_proxy_mode_title,
  [WidgetId.RecentTraffic]: m.dashboard_widget_recent_traffic_title,
  [WidgetId.OriginTraffic]: m.dashboard_widget_origin_traffic_title,
  [WidgetId.ExitTraffic]: m.dashboard_widget_exit_traffic_title,
  [WidgetId.TargetTraffic]: m.dashboard_widget_target_traffic_title,
  [WidgetId.RuleTraffic]: m.dashboard_widget_rule_traffic_title,
  [WidgetId.ActiveConnections]: m.dashboard_widget_active_connections_title,
  [WidgetId.ProviderUpdates]: m.dashboard_widget_provider_updates_title,
}

// The icon each widget shows beside its own title.
const WIDGET_ICON_MAP: Record<
  WidgetId,
  ComponentType<{ className?: string }>
> = {
  [WidgetId.TrafficDown]: ArrowDownwardRounded,
  [WidgetId.TrafficUp]: ArrowUpwardRounded,
  [WidgetId.Connections]: SettingsEthernetRounded,
  [WidgetId.Memory]: MemoryOutlineRounded,
  [WidgetId.ProxyShortcuts]: SettingsEthernetRounded,
  [WidgetId.CoreShortcuts]: MemoryOutlineRounded,
  [WidgetId.SubscriptionQuota]: DataUsageRounded,
  [WidgetId.SubscriptionSchedule]: EventRepeatRounded,
  [WidgetId.ProxyMode]: RouteRounded,
  [WidgetId.RecentTraffic]: DataUsageRounded,
  [WidgetId.OriginTraffic]: AppsRounded,
  [WidgetId.ExitTraffic]: CallSplitRounded,
  [WidgetId.TargetTraffic]: DnsRounded,
  [WidgetId.RuleTraffic]: RuleFolderRounded,
  [WidgetId.ActiveConnections]: NetworkCheckRounded,
  [WidgetId.ProviderUpdates]: CloudSyncRounded,
}

const widgetEntryAddVariants = cva('size-9 shrink-0 shadow-none', {
  variants: {
    placed: {
      true: 'bg-secondary-container text-on-secondary-container hover:bg-secondary-container',
      false: 'bg-surface-variant text-on-surface hover:bg-surface-variant',
    },
  },
})

// The size a widget takes when it is added, as placeWidget prefers it.
function widgetSize(id: WidgetId) {
  const { minW, minH } = WIDGET_MIN_SIZE_MAP[id]
  return WIDGET_RECOMMENDED_SIZE_MAP[id] ?? { w: minW, h: minH }
}

function WidgetEntry({
  id,
  placed,
  onAdd,
  onDragStart,
}: {
  id: WidgetId
  placed: boolean
  onAdd: (id: WidgetId) => void
  onDragStart: () => void
}) {
  const title = WIDGET_TITLE_MAP[id]()
  const Icon = WIDGET_ICON_MAP[id]
  const { w, h } = widgetSize(id)

  return (
    <DndGridSource
      itemId={id}
      dragIdPrefix="sheet:"
      w={w}
      h={h}
      onSourceDragStart={onDragStart}
      className="bg-surface-variant/40 flex min-w-0 items-center gap-3 rounded-2xl py-2 pr-2 pl-4"
      data-slot="widget-sheet-entry"
      data-placed={placed || undefined}
    >
      <Icon className="text-on-surface-variant size-5 shrink-0" />

      <TextMarquee className="min-w-0 flex-1 text-sm">{title}</TextMarquee>

      <span className="text-on-surface-variant text-xs tabular-nums">
        {w}×{h}
      </span>

      <Button
        variant="raised"
        icon
        className={widgetEntryAddVariants({ placed })}
        aria-label={m.dashboard_widget_library_add_entry({ name: title })}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={() => onAdd(id)}
      >
        {placed ? (
          <DoneRounded className="size-5" />
        ) : (
          <AddRounded className="size-5" />
        )}
      </Button>
    </DndGridSource>
  )
}

export function WidgetSheet({
  placedTypes,
  onAdd,
}: {
  placedTypes: ReadonlySet<WidgetId>
  onAdd: (id: WidgetId) => void
}) {
  const { openSheet, setOpenSheet } = useDashboardContext()

  const [search, setSearch] = useState('')

  const query = search.trim().toLocaleLowerCase()
  const filteredIds = (Object.keys(RENDER_MAP) as WidgetId[]).filter(
    (id) =>
      !query || WIDGET_TITLE_MAP[id]().toLocaleLowerCase().includes(query),
  )

  // Close the library so the new widget appears on the dashboard in view.
  const handleAdd = (id: WidgetId) => {
    onAdd(id)
    setOpenSheet(false)
  }

  return (
    <Drawer open={openSheet} onOpenChange={setOpenSheet}>
      <DrawerContent
        className="max-h-[85dvh] min-h-0 w-[calc(100%-1rem)] max-w-[960px]"
        aria-describedby={undefined}
      >
        <DrawerHandle />

        <div className="shrink-0 px-4 pt-2 pb-3">
          <div className="flex items-center justify-between gap-4">
            <DrawerTitle className="text-lg font-semibold">
              {m.dashboard_add_widget()}
            </DrawerTitle>

            <DrawerClose asChild>
              <Button
                variant="raised"
                className="size-8"
                icon
                aria-label={m.dashboard_widget_config_close_library()}
              >
                <CloseRounded className="size-4" />
              </Button>
            </DrawerClose>
          </div>

          <SearchField
            className="mt-3"
            value={search}
            onValueChange={setSearch}
            placeholder={m.dashboard_widget_library_search()}
            clearLabel={m.dashboard_widget_library_search_clear()}
          />
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div
            className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-2 px-4 pb-4"
            data-slot="widget-sheet-library"
          >
            {filteredIds.map((id) => (
              <WidgetEntry
                key={id}
                id={id}
                placed={placedTypes.has(id)}
                onAdd={handleAdd}
                onDragStart={() => setOpenSheet(false)}
              />
            ))}

            {filteredIds.length === 0 && (
              <EmptyState
                tone="normal"
                className="col-span-full"
                message={m.dashboard_widget_library_no_results()}
              />
            )}
          </div>
        </ScrollArea>
      </DrawerContent>
    </Drawer>
  )
}

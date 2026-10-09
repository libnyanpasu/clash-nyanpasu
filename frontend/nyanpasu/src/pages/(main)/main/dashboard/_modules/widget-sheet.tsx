import AddRounded from '~icons/material-symbols/add-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DndGrid,
  useDndGridContext,
  type DndGridItemType,
  type GridSize,
} from '@nyanpasu/ui/dnd-grid'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerTitle,
} from '@nyanpasu/ui/drawer'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WIDGET_RECOMMENDED_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts'
import { useDashboardContext } from '@/components/widgets/provider'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { layoutWidgetSheet } from './widget-sheet-layout'

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

const PREVIEW_CELL_SIZE = 64
const PREVIEW_GAP = 16
const MIN_GRID_COLUMNS = Math.max(
  ...Object.values(WIDGET_MIN_SIZE_MAP).map(({ minW }) => minW),
)

function SheetWidget({
  id,
  onAdd,
}: {
  id: WidgetId
  onAdd: (id: WidgetId) => void
}) {
  const { displayItems, getItemRect, isOverlay } = useDndGridContext()
  const item = displayItems.find((entry) => entry.id === id)
  const rect = item ? getItemRect(item) : null
  const WidgetComponent = RENDER_MAP[id]
  return (
    <>
      <WidgetComponent id={id} />
      {rect && !isOverlay && (
        <Button
          variant="raised"
          icon
          className="absolute z-10 size-7"
          style={{ left: rect.left + rect.width - 32, top: rect.top + 4 }}
          aria-label={m.dashboard_add_widget()}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => onAdd(id)}
        >
          <AddRounded className="size-4" />
        </Button>
      )}
    </>
  )
}

export function WidgetSheet({
  onAdd,
  onSourceDragStart,
}: {
  onAdd: (id: WidgetId) => void
  onSourceDragStart: () => void
}) {
  const { openSheet, setOpenSheet } = useDashboardContext()

  const [search, setSearch] = useState('')
  const [gridSize, setGridSize] = useState<GridSize>()

  const query = search.trim().toLocaleLowerCase()
  const filteredIds = (Object.keys(RENDER_MAP) as WidgetId[]).filter(
    (id) =>
      !query || WIDGET_TITLE_MAP[id]().toLocaleLowerCase().includes(query),
  )

  const gridColumns = gridSize
    ? Math.max(MIN_GRID_COLUMNS, gridSize.cols)
    : undefined

  const sheetItems: DndGridItemType<WidgetId>[] = gridColumns
    ? layoutWidgetSheet(
        filteredIds,
        gridColumns,
        WIDGET_MIN_SIZE_MAP,
        WIDGET_RECOMMENDED_SIZE_MAP,
      )
    : []

  const contentRows = sheetItems.reduce(
    (rows, item) => Math.max(rows, item.y + item.h),
    1,
  )

  return (
    <Drawer open={openSheet} onOpenChange={setOpenSheet}>
      <DrawerContent
        className="h-[85dvh] max-h-[85dvh] min-h-0 w-[calc(100%-1rem)] max-w-[960px]"
        aria-describedby={undefined}
      >
        <div className="shrink-0 p-4 pb-3">
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

        <ScrollArea
          className={cn(
            'min-h-0 flex-1',
            '[&_[data-slot=scroll-area-viewport]>div]:block!',
            '[&_[data-slot=scroll-area-viewport]>div]:h-full',
          )}
        >
          <div
            className="relative flex w-full flex-col px-3"
            style={{
              height: Math.max(
                PREVIEW_CELL_SIZE,
                contentRows * (PREVIEW_CELL_SIZE + PREVIEW_GAP) - PREVIEW_GAP,
              ),
            }}
          >
            <DndGrid
              gridId="sheet"
              className="min-h-0 flex-1"
              items={sheetItems}
              minCellSize={64}
              gap={PREVIEW_GAP}
              size={
                gridColumns
                  ? { cols: gridColumns, rows: contentRows }
                  : undefined
              }
              disabled={false}
              sourceOnly
              dragIdPrefix="sheet:"
              onSourceDragStart={onSourceDragStart}
              onSizeChange={(size) => setGridSize(size)}
            >
              {(item) => {
                return <SheetWidget id={item.id as WidgetId} onAdd={onAdd} />
              }}
            </DndGrid>
            {filteredIds.length === 0 && (
              <p className="text-on-surface-variant pointer-events-none absolute inset-0 grid place-items-center text-sm">
                {m.dashboard_widget_library_no_results()}
              </p>
            )}
          </div>
        </ScrollArea>
      </DrawerContent>
    </Drawer>
  )
}

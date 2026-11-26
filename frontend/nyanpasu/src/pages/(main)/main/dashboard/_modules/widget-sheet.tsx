import AddRounded from '~icons/material-symbols/add-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import DoneRounded from '~icons/material-symbols/done-rounded'
import { cva } from 'class-variance-authority'
import { useMemo, useState } from 'react'
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
import { WidgetPreview, type WidgetDefinition } from '@/components/widgets/core'
import { useDashboardContext } from '@/components/widgets/provider'
import {
  WIDGET_CATEGORIES,
  WIDGET_LIST,
  type WidgetCategoryFilter,
} from '@/components/widgets/registry'
import { WidgetId } from '@/components/widgets/widget-config'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

const categoryItemVariants = cva(
  [
    'flex w-full items-center gap-2.5 rounded-2xl px-3 py-2 text-sm font-medium',
    'outline-none transition-colors focus-visible:ring-2 focus-visible:ring-primary',
  ],
  {
    variants: {
      active: {
        true: 'bg-secondary-container text-on-secondary-container',
        false: 'text-on-surface-variant hover:bg-on-surface/8',
      },
    },
  },
)

const widgetEntryAddVariants = cva('size-9 shrink-0 shadow-none', {
  variants: {
    placed: {
      true: 'bg-secondary-container text-on-secondary-container hover:bg-secondary-container',
      false: 'bg-surface-container-high text-on-surface hover:bg-on-surface/8',
    },
  },
})

function WidgetPreviewCard({
  definition,
  placed,
  onAdd,
  onDragStart,
}: {
  definition: WidgetDefinition
  placed: boolean
  onAdd: (id: WidgetId) => void
  onDragStart: () => void
}) {
  const title = definition.title()
  const Icon = definition.icon
  const { defaultW, defaultH } = definition.size

  return (
    <DndGridSource
      itemId={definition.id}
      dragIdPrefix="sheet:"
      w={defaultW}
      h={defaultH}
      onSourceDragStart={onDragStart}
      data-slot="widget-sheet-entry"
      data-placed={placed || undefined}
      className={cn(
        'bg-surface-container-high border-outline-variant/40 flex min-w-0 flex-col overflow-hidden rounded-3xl border',
        'hover:border-outline-variant transition-colors',
      )}
    >
      <div className="bg-surface-container-high relative h-40 w-full shrink-0 overflow-hidden p-3">
        <WidgetPreview definition={definition} />

        <span className="bg-surface/80 text-on-surface-variant absolute top-2.5 right-2.5 rounded-full px-2 py-0.5 text-xs tabular-nums backdrop-blur-sm">
          {defaultW}×{defaultH}
        </span>
      </div>

      <div className="flex min-w-0 items-center gap-3 px-4 py-3">
        <Icon
          className="text-on-surface-variant size-5 shrink-0"
          aria-hidden="true"
        />

        <div className="min-w-0 flex-1">
          <TextMarquee className="text-sm font-medium">{title}</TextMarquee>
          <p className="text-on-surface-variant truncate text-xs">
            {definition.description()}
          </p>
        </div>

        <Button
          variant="raised"
          icon
          className={widgetEntryAddVariants({ placed })}
          aria-label={m.dashboard_widget_library_add_entry({ name: title })}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={() => onAdd(definition.id)}
        >
          {placed ? (
            <DoneRounded className="size-5" />
          ) : (
            <AddRounded className="size-5" />
          )}
        </Button>
      </div>
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
  const [category, setCategory] = useState<WidgetCategoryFilter>('all')
  const [search, setSearch] = useState('')

  const query = search.trim().toLocaleLowerCase()

  // Filter before counting so the sidebar reports matches under the query.
  const searchMatched = useMemo(
    () =>
      WIDGET_LIST.filter((definition) => {
        if (!query) return true

        return (
          definition.title().toLocaleLowerCase().includes(query) ||
          definition.description().toLocaleLowerCase().includes(query)
        )
      }),
    [query],
  )

  const countsByCategory = useMemo(() => {
    const counts = new Map<WidgetCategoryFilter, number>([
      ['all', searchMatched.length],
    ])

    for (const definition of searchMatched) {
      counts.set(
        definition.category,
        (counts.get(definition.category) ?? 0) + 1,
      )
    }

    return counts
  }, [searchMatched])

  const visibleWidgets = useMemo(
    () =>
      category === 'all'
        ? searchMatched
        : searchMatched.filter(
            (definition) => definition.category === category,
          ),
    [category, searchMatched],
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

        <div className="border-outline-variant/40 shrink-0 border-b px-4 pt-2 pb-3">
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

          <nav
            aria-label={m.dashboard_widget_category_nav()}
            data-slot="widget-sheet-categories-mobile"
            className="mt-3 flex min-w-0 gap-1.5 overflow-x-auto pb-0.5 md:hidden"
          >
            {WIDGET_CATEGORIES.map((entry) => {
              const Icon = entry.icon

              return (
                <button
                  key={entry.id}
                  type="button"
                  aria-pressed={category === entry.id}
                  onClick={() => setCategory(entry.id)}
                  className={cn(
                    categoryItemVariants({ active: category === entry.id }),
                    'w-auto shrink-0 px-3 py-1.5 text-xs',
                  )}
                >
                  <Icon className="size-4 shrink-0" aria-hidden="true" />
                  <span>{entry.label()}</span>
                  <span className="tabular-nums opacity-70">
                    {countsByCategory.get(entry.id) ?? 0}
                  </span>
                </button>
              )
            })}
          </nav>
        </div>

        <div className="flex min-h-0 flex-1 overflow-hidden">
          <aside
            aria-label={m.dashboard_widget_category_nav()}
            data-slot="widget-sheet-sidebar"
            className="border-outline-variant/40 hidden w-48 shrink-0 flex-col gap-1 border-r p-3 md:flex"
          >
            {WIDGET_CATEGORIES.map((entry) => {
              const Icon = entry.icon

              return (
                <button
                  key={entry.id}
                  type="button"
                  aria-current={category === entry.id ? 'page' : undefined}
                  onClick={() => setCategory(entry.id)}
                  className={categoryItemVariants({
                    active: category === entry.id,
                  })}
                >
                  <Icon className="size-4 shrink-0" aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate text-left">
                    {entry.label()}
                  </span>
                  <span className="text-xs tabular-nums opacity-70">
                    {countsByCategory.get(entry.id) ?? 0}
                  </span>
                </button>
              )
            })}
          </aside>

          <ScrollArea className="min-h-0 flex-1">
            <div
              data-slot="widget-sheet-library"
              className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-2"
            >
              {visibleWidgets.map((definition) => (
                <WidgetPreviewCard
                  key={definition.id}
                  definition={definition}
                  placed={placedTypes.has(definition.id)}
                  onAdd={handleAdd}
                  onDragStart={() => setOpenSheet(false)}
                />
              ))}

              {visibleWidgets.length === 0 && (
                <EmptyState
                  tone="normal"
                  className="col-span-full"
                  message={m.dashboard_widget_library_no_results()}
                >
                  {category !== 'all' && searchMatched.length > 0 && (
                    <Button variant="raised" onClick={() => setCategory('all')}>
                      {m.dashboard_widget_library_show_all_results({
                        count: String(searchMatched.length),
                      })}
                    </Button>
                  )}
                </EmptyState>
              )}
            </div>
          </ScrollArea>
        </div>
      </DrawerContent>
    </Drawer>
  )
}

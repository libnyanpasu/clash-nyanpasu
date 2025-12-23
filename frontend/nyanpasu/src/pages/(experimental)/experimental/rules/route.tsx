import {
  AppContentScrollArea,
  useScrollArea,
} from '@/components/ui/scroll-area'
import { useClashRules } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

export const Route = createFileRoute('/(experimental)/experimental/rules')({
  component: RouteComponent,
})

const InnerComponent = () => {
  const { data } = useClashRules()

  const rules = data?.rules

  const { viewportRef } = useScrollArea()

  const rowVirtualizer = useVirtualizer({
    count: rules?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = rowVirtualizer.getVirtualItems()

  return (
    <div
      className="relative mx-4 flex flex-col"
      data-slot="rules-virtual-list"
      style={{
        height: `${rowVirtualizer.getTotalSize()}px`,
      }}
    >
      {virtualItems.map((virtualItem) => {
        const rule = rules?.[virtualItem.index]

        if (!rule) {
          return null
        }

        return (
          <div
            key={virtualItem.key}
            ref={rowVirtualizer.measureElement}
            className={cn(
              'absolute top-0 left-0 w-full select-text',
              'font-mono break-all',
              'flex items-center gap-2 py-2',
            )}
            data-index={virtualItem.index}
            data-slot="rules-virtual-item"
            style={{
              transform: `translateY(${virtualItem.start}px)`,
            }}
          >
            <div className="min-w-14">{virtualItem.index + 1}</div>

            <div className="flex flex-col gap-1">
              <div className="text-primary">{rule.payload || '-'}</div>

              <div className="flex gap-8">
                <div className="min-w-40 text-sm">{rule.type}</div>

                <div className="text-s text-sm">{rule.proxy}</div>
              </div>
            </div>
          </div>
        )
      })}
    </div>
  )
}

function RouteComponent() {
  return (
    <AppContentScrollArea>
      <InnerComponent />
    </AppContentScrollArea>
  )
}

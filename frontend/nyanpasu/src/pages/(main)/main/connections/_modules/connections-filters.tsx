import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { dimensionName } from '@/utils/traffic-usage'
import { cn } from '@nyanpasu/utils'
import FilterChip from '../../_modules/filter-chip'
import { rangeName } from '../../_modules/traffic-filters'
import { useUsageLabelOf } from '../../_modules/use-usage-label-of'
import type { ConnectionsSelection } from './use-connection-rows'

/** The selection another page brought, as chips that remove its conditions. */
export default function ConnectionsFilters({
  selection,
  onSelectionChange,
  className,
}: {
  selection: ConnectionsSelection
  onSelectionChange: (selection: ConnectionsSelection) => void
  className?: string
}) {
  const labelOf = useUsageLabelOf()

  const { range, filters } = selection

  return (
    <div
      className={cn(
        'flex min-w-0 [scrollbar-width:none] items-center gap-2 overflow-x-auto',
        className,
      )}
      data-slot="connections-filters"
      role="group"
      aria-label={m.traffic_filters_label()}
    >
      {filters.map((filter) => {
        const label = labelOf(filter.d, filter.v)

        return (
          <FilterChip
            key={filter.d}
            label={`${dimensionName(filter.d)}: ${label.text}`}
            title={label.title}
            onRemove={() =>
              onSelectionChange({
                range,
                filters: filters.filter((item) => item !== filter),
              })
            }
          />
        )
      })}

      {range !== undefined && (
        <FilterChip
          label={`${m.traffic_range_label()}: ${rangeName(range)}`}
          onRemove={() => onSelectionChange({ range: undefined, filters })}
        />
      )}

      <Button
        className="h-8 shrink-0 px-3 whitespace-nowrap"
        onClick={() => onSelectionChange({ range: undefined, filters: [] })}
      >
        {m.traffic_filter_clear_all()}
      </Button>
    </div>
  )
}

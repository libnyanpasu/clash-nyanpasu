import { m } from '@/paraglide/messages'
import { dimensionName } from '@/utils/traffic-usage'
import FilterChips, { type FilterChipItem } from '../../_modules/filter-chips'
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

  const items: FilterChipItem[] = filters.map((filter) => {
    const label = labelOf(filter.d, filter.v)

    return {
      key: filter.d,
      label: `${dimensionName(filter.d)}: ${label.text}`,
      title: label.title,
      onRemove: () =>
        onSelectionChange({
          range,
          filters: filters.filter((item) => item !== filter),
        }),
    }
  })

  if (range !== undefined) {
    items.push({
      key: 'range',
      label: `${m.traffic_range_label()}: ${rangeName(range)}`,
      onRemove: () => onSelectionChange({ range: undefined, filters }),
    })
  }

  return (
    <FilterChips
      className={className}
      data-slot="connections-filters"
      items={items}
      onClearAll={() => onSelectionChange({ range: undefined, filters: [] })}
    />
  )
}

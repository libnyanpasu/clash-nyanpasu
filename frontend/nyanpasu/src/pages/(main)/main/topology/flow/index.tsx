import ArrowDropDownRounded from '~icons/material-symbols/arrow-drop-down-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Card } from '@nyanpasu/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { dimensionName } from '@/utils/traffic-usage'
import { createFileRoute } from '@tanstack/react-router'
import {
  DEFAULT_TEMPLATE,
  LAYER_TEMPLATES,
  LIMITS,
  TEMPLATES,
  type LayerTemplate,
  type Limit,
} from '../_modules/search'
import { useTrafficPage } from '../_modules/traffic-context'
import { Sankey } from './_modules/sankey'

export const Route = createFileRoute('/(main)/main/topology/flow/')({
  component: TrafficFlowPage,
})

const limitName = (limit: Limit) =>
  limit === 'all'
    ? m.traffic_limit_all()
    : m.traffic_limit_top({ count: limit })

const templateName = (template: LayerTemplate) =>
  LAYER_TEMPLATES[template].map(dimensionName).join(' → ')

function TrafficFlowPage() {
  const { search, report, onSearchChange } = useTrafficPage()
  const topology = report?.topology
  const { layers: layerTemplate, limit } = search
  const template = layerTemplate ?? DEFAULT_TEMPLATE
  const layers = LAYER_TEMPLATES[template]

  return (
    <section
      className="flex min-h-0 flex-1 flex-col"
      aria-label={m.topology_title()}
    >
      <Card
        className="flex min-h-80 flex-1 flex-col p-3 md:p-4"
        data-slot="flow-card"
      >
        <div className="mb-4 flex min-w-0 flex-wrap items-end gap-3">
          <div className="w-full sm:w-64">
            <DropdownMenu width="full">
              <DropdownMenuTrigger asChild>
                <Button
                  className="flex h-9 w-full min-w-0 items-center justify-between gap-1 px-3 py-1"
                  aria-label={m.traffic_layers_label()}
                >
                  <span className="min-w-0 flex-1 truncate text-left">
                    {templateName(template)}
                  </span>
                  <ArrowDropDownRounded
                    aria-hidden
                    className="size-5 shrink-0"
                  />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="min-w-64">
                <DropdownMenuRadioGroup
                  value={template}
                  onValueChange={(next) =>
                    onSearchChange({ layers: next as LayerTemplate })
                  }
                >
                  {TEMPLATES.map((value) => (
                    <DropdownMenuRadioItem key={value} value={value}>
                      {templateName(value)}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <div className="w-full sm:w-36">
            <DropdownMenu width="full">
              <DropdownMenuTrigger asChild>
                <Button
                  className="flex h-9 w-full min-w-0 items-center justify-between gap-1 px-3 py-1"
                  aria-label={m.traffic_limit_label()}
                >
                  <span className="min-w-0 flex-1 truncate text-left">
                    {limitName(limit)}
                  </span>
                  <ArrowDropDownRounded
                    aria-hidden
                    className="size-5 shrink-0"
                  />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent>
                <DropdownMenuRadioGroup
                  value={String(limit)}
                  onValueChange={(next) => {
                    const value = LIMITS.find((item) => String(item) === next)

                    if (value !== undefined) onSearchChange({ limit: value })
                  }}
                >
                  {LIMITS.map((value) => (
                    <DropdownMenuRadioItem key={value} value={String(value)}>
                      {limitName(value)}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        {!topology?.nodes.length ? (
          <div className="text-on-surface-variant grid min-h-64 flex-1 place-content-center text-center">
            <p className="text-lg">
              {topology ? m.traffic_empty() : m.traffic_loading_more()}
            </p>
          </div>
        ) : (
          <Sankey layers={layers} topology={topology} />
        )}
        <p className="text-on-surface-variant mt-3 text-xs leading-relaxed">
          {m.topology_caption()}
        </p>
      </Card>
    </section>
  )
}

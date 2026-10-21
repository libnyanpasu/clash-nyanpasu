import SettingsRounded from '~icons/material-symbols/settings-rounded'
import { memo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import RankingCard from './ranking-card'
import { RANKINGS, useTrafficPage } from './traffic-context'
import {
  FLOW_GRAPHS,
  useTrafficViewSettings,
  type FlowGraph,
} from './view-settings'

const graphName = (graph: FlowGraph) =>
  graph === 'sankey'
    ? m.traffic_flow_graph_sankey()
    : m.traffic_flow_graph_cards()

function FlowGraphOptions() {
  const { settings, onSettingsChange } = useTrafficViewSettings()

  return (
    <>
      <DropdownMenuSeparator />

      <DropdownMenuLabel className="text-on-surface-variant text-sm">
        {m.traffic_flow_graph()}
      </DropdownMenuLabel>

      <DropdownMenuRadioGroup
        value={settings.flowGraph}
        onValueChange={(next) =>
          onSettingsChange({ flowGraph: next as FlowGraph })
        }
      >
        {FLOW_GRAPHS.map((value) => (
          <DropdownMenuRadioItem key={value} value={value}>
            {graphName(value)}
          </DropdownMenuRadioItem>
        ))}
      </DropdownMenuRadioGroup>
    </>
  )
}

// Apart from the report, so a poll leaves the menu alone.
const ViewSettings = memo(function ViewSettings({
  flowGraph,
}: {
  flowGraph: boolean
}) {
  const { settings, onSettingsChange } = useTrafficViewSettings()
  const { rankings } = settings
  const labelled = rankings.length === 0

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        {/* With no ranking shown, the button says what it brings back
            rather than being a lone icon under the drawing. It stays the
            same element, so the open menu keeps its anchor. */}
        <Button
          icon={!labelled}
          className={cn(
            'h-8 shrink-0',
            labelled ? 'inline-flex items-center gap-1.5 px-3 text-sm' : 'w-8',
          )}
          aria-label={labelled ? undefined : m.traffic_view_settings()}
        >
          <SettingsRounded
            aria-hidden
            className={labelled ? 'size-4' : 'size-5'}
          />
          {labelled && m.traffic_view_rankings_show()}
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent className="min-w-56">
        <DropdownMenuLabel className="text-on-surface-variant text-sm">
          {m.traffic_view_rankings_shown()}
        </DropdownMenuLabel>

        {RANKINGS.map(({ dimension, title }) => (
          <DropdownMenuCheckboxItem
            key={dimension}
            checked={rankings.includes(dimension)}
            onCheckedChange={(checked) =>
              onSettingsChange({
                rankings: checked
                  ? [...rankings, dimension]
                  : rankings.filter((item) => item !== dimension),
              })
            }
            // The menu stays open to pick several.
            onSelect={(event) => event.preventDefault()}
          >
            {title()}
          </DropdownMenuCheckboxItem>
        ))}

        {flowGraph && <FlowGraphOptions />}
      </DropdownMenuContent>
    </DropdownMenu>
  )
})

/**
 * The rankings chosen in its settings under a drawing. A node or a region
 * picked in the drawing filters them in place, so its result reads on the
 * same page. With none chosen, only the way to choose them is left.
 */
export default function ViewRankings({
  flowGraph = false,
}: {
  /** Whether the settings also choose the flow page's drawing. */
  flowGraph?: boolean
}) {
  const { report, empty } = useTrafficPage()
  const { rankings } = useTrafficViewSettings().settings
  const shown = rankings.length > 0

  if (!report || empty) return null

  return (
    <section
      className="mt-3 flex flex-col gap-2"
      aria-label={m.traffic_view_rankings()}
      data-slot="traffic-view-rankings"
    >
      <div
        className={cn(
          'flex min-w-0 items-center gap-2 px-1',
          shown ? 'justify-between' : 'justify-end',
        )}
      >
        {shown && (
          <h2 className="text-on-surface-variant truncate text-sm font-medium">
            {m.traffic_view_rankings()}
          </h2>
        )}

        <ViewSettings flowGraph={flowGraph} />
      </div>

      {shown && (
        <div className="grid grid-cols-1 items-start gap-3 @2xl:grid-cols-2 @5xl:grid-cols-3">
          {RANKINGS.map(({ dimension, icon, title }) => {
            const ranking =
              rankings.includes(dimension) &&
              report.rankings.find((item) => item.dimension === dimension)

            return (
              ranking && (
                <RankingCard
                  key={dimension}
                  dimension={dimension}
                  icon={icon}
                  title={title()}
                  ranking={ranking}
                />
              )
            )
          })}
        </div>
      )}
    </section>
  )
}

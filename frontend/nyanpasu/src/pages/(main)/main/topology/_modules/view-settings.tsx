import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  type PropsWithChildren,
} from 'react'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { useKvStorage } from '@nyanpasu/query'
import { RANKINGS, type RankingDimension } from './traffic-context'

/** The drawings the flow page can show its paths as. */
export const FLOW_GRAPHS = ['sankey', 'cards'] as const

export type FlowGraph = (typeof FLOW_GRAPHS)[number]

/** How the flow and the map pages are shown, the same on both. */
export type TrafficViewSettings = {
  /** The rankings under the drawing, in the order of `RANKINGS`. */
  rankings: RankingDimension[]
  flowGraph: FlowGraph
}

export const TRAFFIC_VIEW_KV_KEY = 'traffic-view'

export const DEFAULT_TRAFFIC_VIEW: TrafficViewSettings = {
  // The most asked-for rankings.
  rankings: RANKINGS.slice(0, 3).map(({ dimension }) => dimension),
  flowGraph: 'sankey',
}

/** Coerces whatever the KV store holds into the view settings. */
export function toTrafficViewSettings(value: unknown): TrafficViewSettings {
  const stored = (value ?? {}) as Partial<
    Record<keyof TrafficViewSettings, unknown>
  >

  return {
    rankings: Array.isArray(stored.rankings)
      ? RANKINGS.map(({ dimension }) => dimension).filter((dimension) =>
          (stored.rankings as unknown[]).includes(dimension),
        )
      : DEFAULT_TRAFFIC_VIEW.rankings,
    flowGraph: FLOW_GRAPHS.includes(stored.flowGraph as FlowGraph)
      ? (stored.flowGraph as FlowGraph)
      : DEFAULT_TRAFFIC_VIEW.flowGraph,
  }
}

// Apart from the page context, so a change re-renders only what reads it.
const TrafficViewSettingsContext = createContext<{
  settings: TrafficViewSettings
  onSettingsChange: (update: Partial<TrafficViewSettings>) => void
} | null>(null)

export function useTrafficViewSettings() {
  const context = useContext(TrafficViewSettingsContext)

  if (!context)
    throw new Error('Traffic pages must render inside the traffic layout')

  return context
}

/**
 * Reads the view settings once for all the traffic pages, which keep them
 * while they switch. The store's own state stays in here, so reading it
 * leaves the layout alone.
 */
export function TrafficViewSettingsProvider({ children }: PropsWithChildren) {
  const [settings, setSettings] = useKvStorage(
    TRAFFIC_VIEW_KV_KEY,
    DEFAULT_TRAFFIC_VIEW,
    { migrate: toTrafficViewSettings },
  )

  const onSettingsChange = useCallback(
    async (update: Partial<TrafficViewSettings>) => {
      if (!(await setSettings((previous) => ({ ...previous, ...update })))) {
        message(m.traffic_view_settings_save_failed(), { kind: 'error' })
      }
    },
    [setSettings],
  )
  const value = useMemo(
    () => ({ settings, onSettingsChange }),
    [settings, onSettingsChange],
  )

  return (
    <TrafficViewSettingsContext value={value}>
      {children}
    </TrafficViewSettingsContext>
  )
}

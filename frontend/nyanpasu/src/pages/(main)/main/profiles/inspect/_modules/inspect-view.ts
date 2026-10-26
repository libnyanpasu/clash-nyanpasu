import type { InspectTab } from './step-detail'

/** How the inspector was last left, kept across visits. */
export type InspectView = {
  /**
   * The document tab last chosen, which every step opens in; until one is
   * chosen, a step opens in its own default. The logs tab is not kept.
   */
  documentTab: Exclude<InspectTab, 'logs'> | null
  /** Whether the chain shows only the steps that changed something. */
  onlyChanged: boolean
}

export const INSPECT_VIEW_KV_KEY = 'inspect-view'

export const DEFAULT_INSPECT_VIEW: InspectView = {
  documentTab: null,
  onlyChanged: false,
}

/** Coerces whatever the KV store holds into an InspectView. */
export function toInspectView(value: unknown): InspectView {
  const stored = (value ?? {}) as Partial<Record<keyof InspectView, unknown>>

  return {
    documentTab:
      stored.documentTab === 'changes' || stored.documentTab === 'output'
        ? stored.documentTab
        : DEFAULT_INSPECT_VIEW.documentTab,
    onlyChanged:
      typeof stored.onlyChanged === 'boolean'
        ? stored.onlyChanged
        : DEFAULT_INSPECT_VIEW.onlyChanged,
  }
}

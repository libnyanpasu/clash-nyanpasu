import {
  createContext,
  PropsWithChildren,
  use,
  useCallback,
  useRef,
  useState,
} from 'react'
import { useDndGridContext } from '@/components/ui/dnd-grid/context'
import { useKvStorage } from '@nyanpasu/interface'
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  getWidgetConfig,
  normalizeWidgetConfigStorage,
  WidgetConfig,
  WidgetConfigStorage,
  WidgetId,
} from './widget-config'

type SaveStatus = { state: 'saved' | 'saving' | 'error'; id?: string }

const DashboardContext = createContext<{
  openSheet: boolean
  setOpenSheet: (open: boolean) => void
  isEditing: boolean
  setIsEditing: (editing: boolean) => void
  configs: WidgetConfigStorage
  saveConfig: (id: string, config: WidgetConfig) => void
  saveStatus: SaveStatus
  configLoading: boolean
  configReadError: boolean
  retryConfig: () => void
} | null>(null)

export const useDashboardContext = () => {
  const context = use(DashboardContext)

  if (!context) {
    throw new Error(
      'useDashboardContext must be used within a DashboardProvider',
    )
  }

  return context
}

export function DashboardProvider({ children }: PropsWithChildren) {
  const [openSheet, setOpenSheet] = useState(false)

  const [isEditing, setIsEditing] = useState(false)

  const [configs, setConfigs, { isLoading, readError, refresh }] = useKvStorage(
    'dashboard-widget-configs',
    EMPTY_WIDGET_CONFIG_STORAGE,
    { migrate: normalizeWidgetConfigStorage },
  )
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ state: 'saved' })

  const configsRef = useRef(configs)
  configsRef.current = configs
  const savingRef = useRef(false)

  const persist = useCallback(
    async (id: string | undefined, next: WidgetConfigStorage) => {
      if (savingRef.current) return
      savingRef.current = true
      setSaveStatus({ state: 'saving', id })
      const saved = await setConfigs(next)
      savingRef.current = false
      setSaveStatus({ state: saved ? 'saved' : 'error', id })
    },
    [setConfigs],
  )

  const saveConfig = useCallback(
    (id: string, config: WidgetConfig) => {
      // Do not overwrite the authoritative first read or race full-map writes.
      if (isLoading || readError !== null || savingRef.current) return
      const next: WidgetConfigStorage = {
        version: 1,
        byInstance: { ...configsRef.current.byInstance, [id]: config },
      }
      configsRef.current = next
      persist(id, next)
    },
    [isLoading, readError, persist],
  )

  const retryConfig = useCallback(() => {
    if (readError !== null) refresh()
    else persist(saveStatus.id, configsRef.current)
  }, [readError, refresh, persist, saveStatus.id])

  return (
    <DashboardContext.Provider
      value={{
        openSheet,
        setOpenSheet,
        isEditing,
        setIsEditing,
        configs,
        saveConfig,
        saveStatus,
        configLoading: isLoading,
        configReadError: readError !== null,
        retryConfig,
      }}
    >
      {children}
    </DashboardContext.Provider>
  )
}

export function useWidgetConfig<T extends WidgetId>(id: string, type: T) {
  const { configs } = useDashboardContext()
  const { sourceOnly, isOverlay, dragIdPrefix } = useDndGridContext()

  return sourceOnly && (!isOverlay || dragIdPrefix !== '')
    ? DEFAULT_WIDGET_CONFIGS[type]
    : getWidgetConfig(configs, id, type)
}

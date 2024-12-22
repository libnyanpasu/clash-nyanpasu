import { atom } from 'jotai'
import { atomWithStorage, createJSONStorage } from 'jotai/utils'
import { SortType } from '@/components/proxies/utils'
import { FileRouteTypes } from '@/routeTree.gen'
import { NyanpasuStorage } from '@/services/storage'
import { LogMessage } from '@nyanpasu/interface'

const atomWithLocalStorage = <T>(key: string, initialValue: T) => {
  const getInitialValue = (): T => {
    const item = localStorage.getItem(key)

    return item ? JSON.parse(item) : initialValue
  }

  const baseAtom = atom<T>(getInitialValue())

  const derivedAtom = atom(
    (get) => get(baseAtom),
    (get, set, update: T | ((prev: T) => T)) => {
      const nextValue =
        typeof update === 'function'
          ? (update as (prev: T) => T)(get(baseAtom))
          : update

      set(baseAtom, nextValue)

      localStorage.setItem(key, JSON.stringify(nextValue))
    },
  )

  return derivedAtom
}

export const memorizedRoutePathAtom = atomWithStorage<
  FileRouteTypes['fullPaths'] | null
>('memorizedRoutePathAtom', null)

export const proxyGroupAtom = atomWithLocalStorage<{
  selector: number | null
}>('proxyGroupAtom', {
  selector: 0,
})

export const proxyGroupSortAtom = atomWithLocalStorage<SortType>(
  'proxyGroupSortAtom',
  SortType.Default,
)

export const themeMode = atomWithLocalStorage<'light' | 'dark'>(
  'themeMode',
  'light',
)

export const atomLogData = atomWithLocalStorage<LogMessage[]>('atomLogData', [])

export const atomEnableLog = atomWithLocalStorage<boolean>(
  'atomEnableLog',
  true,
)

export const atomIsDrawer = atom<boolean>()

export const atomIsDrawerOnlyIcon = atomWithStorage<boolean>(
  'atomIsDrawerOnlyIcon',
  true,
)

// save the state of each profile item loading
export const atomLoadingCache = atom<Record<string, boolean>>({})

// save update state
export const atomUpdateState = atom<boolean>(false)

interface IConnectionSetting {
  layout: 'table' | 'list'
}

export const atomConnectionSetting = atom<IConnectionSetting>({
  layout: 'table',
})

// TODO: generate default columns based on COLUMNS
export const connectionTableColumnsAtom = atomWithStorage<
  Array<[string, boolean]>
>(
  'connections_table_columns',
  [
    'host',
    'process',
    'downloaded',
    'uploaded',
    'dl_speed',
    'ul_speed',
    'chains',
    'rule',
    'time',
    'source',
    'destination_ip',
    'destination_asn',
    'type',
  ].map((key) => [key, true]),
  createJSONStorage(() => NyanpasuStorage),
)

// export const themeSchemeAtom = atom<MDYTheme["schemes"] | null>(null);

import { useAtom } from 'jotai'
import { useEffect } from 'react'
import { dispatchStorageValueChanged } from '@/services/storage'
import { coreTypeAtom } from '@/store/clash'
import { useNyanpasu } from '@nyanpasu/interface'
import { listen, UnlistenFn } from '@tauri-apps/api/event'

export function useCoreType() {
  const [coreType, setCoreType] = useAtom(coreTypeAtom)
  const { setNyanpasuConfig } = useNyanpasu({
    onSuccess(data) {
      setCoreType(data?.clash_core || 'mihomo')
    },
  })
  const setter = (value: typeof coreType) => {
    setCoreType(value)
    setNyanpasuConfig({ clash_core: value })
  }
  return [coreType, setter] as const
}

export function useNyanpasuStorageSubscribers() {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    listen<[string, string | null]>('storage_value_changed', (event) => {
      const [key, value] = event.payload
      dispatchStorageValueChanged(
        key,
        typeof value === 'string' ? JSON.parse(value) : value,
      )
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      if (unlisten) {
        unlisten()
      }
    }
  })
}

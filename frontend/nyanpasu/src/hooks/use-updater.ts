import { useAtomValue, useSetAtom } from 'jotai'
import { useEffect, useState } from 'react'
import { OS } from '@/consts'
import { UpdaterIgnoredAtom, UpdaterInstanceAtom } from '@/store/updater'
import { useNyanpasu } from '@nyanpasu/interface'
import { check as checkUpdate } from '@tauri-apps/plugin-updater'
import { useIsAppImage } from './use-consts'

export function useUpdaterPlatformSupported() {
  const [supported, setSupported] = useState(false)
  const isAppImage = useIsAppImage()
  useEffect(() => {
    switch (OS) {
      case 'macos':
      case 'windows':
        setSupported(true)
        break
      case 'linux':
        setSupported(!!isAppImage.data)
        break
    }
  }, [isAppImage.data])
  return supported
}

export default function useUpdater() {
  const { nyanpasuConfig } = useNyanpasu()
  const updaterIgnored = useAtomValue(UpdaterIgnoredAtom)
  const setUpdaterInstance = useSetAtom(UpdaterInstanceAtom)
  const isPlatformSupported = useUpdaterPlatformSupported()

  useEffect(() => {
    const run = async () => {
      if (nyanpasuConfig?.enable_auto_check_update && isPlatformSupported) {
        const updater = await checkUpdate()
        if (updater?.available && updaterIgnored !== updater?.version) {
          setUpdaterInstance(updater || null)
        }
      }
    }
    run().catch(console.error)
  }, [
    isPlatformSupported,
    nyanpasuConfig?.enable_auto_check_update,
    setUpdaterInstance,
    updaterIgnored,
  ])
}

import { atom } from 'jotai'
import { atomWithStorage } from 'jotai/utils'
import { type Update } from '@tauri-apps/plugin-updater'

export const UpdaterIgnoredAtom = atomWithStorage(
  'updaterIgnored',
  null as string | null,
)

export const UpdaterInstanceAtom = atom<Update | null>(null)

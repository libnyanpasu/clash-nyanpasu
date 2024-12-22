import { atom } from 'jotai'

export const serviceManualPromptDialogAtom = atom<
  'install' | 'uninstall' | 'start' | 'stop' | null
>(null)

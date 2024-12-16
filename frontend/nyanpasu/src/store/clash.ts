import { atom } from 'jotai'
import type { VergeConfig } from '@nyanpasu/interface'

export const coreTypeAtom =
  atom<NonNullable<VergeConfig['clash_core']>>('mihomo')

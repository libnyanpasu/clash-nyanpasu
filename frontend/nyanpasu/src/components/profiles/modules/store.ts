import { atom } from 'jotai'
import type { Profile } from '@nyanpasu/interface'

export const atomGlobalChainCurrent = atom<boolean>(false)

export const atomChainsSelected = atom<Profile.Item['uid']>()

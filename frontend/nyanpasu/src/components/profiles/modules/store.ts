import { atom } from 'jotai'

export const atomGlobalChainCurrent = atom<boolean>(false)

export const atomChainsSelected = atom<string>()

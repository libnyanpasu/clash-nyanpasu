import { atom } from 'jotai'
import { RefObject } from 'react'
import { Clash } from '@nyanpasu/interface'

export const atomRulePage = atom<{
  data?: Clash.Rule[]
  scrollRef?: RefObject<HTMLElement>
}>()

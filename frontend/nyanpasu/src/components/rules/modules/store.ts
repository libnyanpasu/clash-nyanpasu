import { atom } from 'jotai'
import { RefObject } from 'react'
import { ClashRule } from '@nyanpasu/interface'

export const atomRulePage = atom<{
  data?: ClashRule[]
  scrollRef?: RefObject<HTMLElement>
}>()

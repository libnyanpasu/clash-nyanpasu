import { atom } from 'jotai'
import { LogMessage } from '@nyanpasu/interface'

export const atomLogList = atom<LogMessage[]>([])

export const atomLogLevel = atom<string>('all')

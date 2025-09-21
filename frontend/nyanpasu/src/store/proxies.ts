import { atomWithStorage } from 'jotai/utils'

export const proxiesFilterAtom = atomWithStorage<string | null>(
  'proxiesFilterAtom',
  null,
)

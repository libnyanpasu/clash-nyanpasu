import type { ClashCore_Serialize } from '@nyanpasu/rpc/types'

/** The core selections that run the Meow engine. */
export type MeowCore = Extract<ClashCore_Serialize, 'meow' | 'meow-alpha'>

export const isMeowCore = (
  core: ClashCore_Serialize | undefined,
): core is MeowCore => core === 'meow' || core === 'meow-alpha'

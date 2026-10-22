import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { unwrapResult } from '@nyanpasu/rpc'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'

/** Reveals the profile's file in the system file manager. */
export const useShowInFileManager = (profile: ProfileItem_Serialize) =>
  useLockFn(async () => {
    try {
      unwrapResult(await rpc.viewProfile(profile.uid))
    } catch (error) {
      await message(formatError(error), {
        title: m.profile_detail_show_in_file_manager(),
        kind: 'error',
        error,
      })
    }
  })

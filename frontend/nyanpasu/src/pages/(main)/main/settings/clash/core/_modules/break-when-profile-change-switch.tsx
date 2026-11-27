import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'

export default function BreakWhenProfileChangeSwitch() {
  const breakConnection = useClashSetting('break_connection')

  const checked = Boolean(breakConnection.value?.on_profile_change)

  const handleChange = useLockFn(async (next: boolean) => {
    try {
      await breakConnection.upsert({ on_profile_change: next })
    } catch (error) {
      message(
        `Update break when profile change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
        },
      )
    }
  })

  return (
    <ItemSwitch
      data-slot="break-when-profile-change-switch"
      title={m.settings_nyanpasu_enhance_break_when_profile_change_label()}
      description={m.settings_nyanpasu_enhance_break_when_profile_change_description()}
      checked={checked}
      onCheckedChange={handleChange}
      loading={breakConnection.isPending}
    />
  )
}

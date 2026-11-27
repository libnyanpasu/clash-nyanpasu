import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'

export default function EnableBuiltinEnhancedSwitch() {
  const enableBuiltinEnhanced = useSetting('enable_builtin_enhanced')

  const handleChange = useLockFn(async (next: boolean) => {
    try {
      await enableBuiltinEnhanced.upsert(next)
    } catch (error) {
      message(
        `Update built-in enhanced failed!\n Error: ${formatError(error)}`,
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
      data-slot="enable-builtin-enhanced-switch"
      title={m.settings_nyanpasu_enhance_enable_builtin_enhanced_label()}
      description={m.settings_nyanpasu_enhance_enable_builtin_enhanced_description()}
      checked={Boolean(enableBuiltinEnhanced.value)}
      onCheckedChange={handleChange}
      loading={enableBuiltinEnhanced.isPending}
    />
  )
}

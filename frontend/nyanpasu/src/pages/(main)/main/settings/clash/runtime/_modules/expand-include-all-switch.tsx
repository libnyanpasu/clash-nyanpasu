import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'

export default function ExpandIncludeAllSwitch() {
  const { value, upsert } = useClashSetting('expand_include_all')

  const handleChange = useLockFn(async (input: boolean) => {
    try {
      await upsert(input)
    } catch (error) {
      message(
        `Update include-all expansion failed!\n Error: ${formatError(error)}`,
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
      data-slot="expand-include-all-switch"
      title={m.settings_clash_settings_expand_include_all_label()}
      description={m.settings_clash_settings_expand_include_all_description()}
      checked={value !== false}
      onCheckedChange={handleChange}
    />
  )
}

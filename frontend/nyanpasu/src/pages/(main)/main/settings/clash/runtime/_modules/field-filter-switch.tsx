import { ItemSwitch } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSystemSetting } from '@nyanpasu/query'

export default function FieldFilterButton() {
  const { value, upsert } = useClashSystemSetting('enable_clash_fields')

  const handleFieldFilter = useLockFn(async (input: boolean) => {
    try {
      await upsert(input)
    } catch (error) {
      message(
        `Activation Field Filter failed!\n Error: ${formatError(error)}`,
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
      data-slot="field-filter-switch-container"
      title={m.settings_clash_settings_field_filter_label()}
      checked={Boolean(value)}
      onCheckedChange={handleFieldFilter}
    />
  )
}

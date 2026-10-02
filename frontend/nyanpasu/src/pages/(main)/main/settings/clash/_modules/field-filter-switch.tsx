import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card'

export default function FieldFilterButton() {
  const { value, upsert } = useClashSetting('enable_clash_fields')

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
    <ItemContainer data-slot="field-filter-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_field_filter_label()}
        </ItemLabelText>
      </ItemLabel>

      <Switch checked={Boolean(value)} onCheckedChange={handleFieldFilter} />
    </ItemContainer>
  )
}

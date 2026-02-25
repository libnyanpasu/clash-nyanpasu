import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function FieldFilterButton() {
  const { value, upsert } = useSetting('enable_clash_fields')

  const handleFieldFilter = useLockFn(async (input: boolean) => {
    try {
      await upsert(input)
    } catch (error) {
      message(
        `Activation Field Filter failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="field-filter-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="field-filter-switch-card-content"
      >
        <div>{m.settings_clash_settings_field_filter_label()}</div>

        <Switch checked={Boolean(value)} onCheckedChange={handleFieldFilter} />
      </SettingsCardContent>
    </SettingsCard>
  )
}

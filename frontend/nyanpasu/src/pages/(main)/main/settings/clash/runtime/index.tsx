import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card'
import EnableBuiltinEnhancedSwitch from './_modules/enable-builtin-enhanced-switch'
import ExpandIncludeAllSwitch from './_modules/expand-include-all-switch'
import FieldFilterCard from './_modules/field-filter-card'
import FieldFilterSwitch from './_modules/field-filter-switch'

export const Route = createFileRoute('/(main)/main/settings/clash/runtime/')({
  component: RouteComponent,
})

const FieldFilterSettings = () => {
  return (
    <div data-slot="field-filter-settings-container">
      <SettingsLabel>
        {m.settings_clash_settings_field_filter_label()}
      </SettingsLabel>

      <div className="space-y-2">
        <SettingsCard>
          <SettingsCardContent>
            <FieldFilterSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <FieldFilterCard />
      </div>
    </div>
  )
}

const ProfileProcessingSettings = () => {
  return (
    <div data-slot="profile-processing-settings-container">
      <SettingsLabel>
        {m.settings_clash_runtime_processing_label()}
      </SettingsLabel>

      <SettingsGroup>
        <ExpandIncludeAllSwitch />

        <EnableBuiltinEnhancedSwitch />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_runtime()}
      </PageTitle>

      <RequireCapability
        permissions={['settings:read', 'profile:read']}
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['settings:write', 'system:write']}>
          <div className="space-y-4 px-4 pb-4">
            <FieldFilterSettings />

            <ProfileProcessingSettings />
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}

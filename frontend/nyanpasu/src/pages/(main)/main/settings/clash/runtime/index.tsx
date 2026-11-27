import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import EnableBuiltinEnhancedSwitch from './_modules/enable-builtin-enhanced-switch'
import ExpandIncludeAllSwitch from './_modules/expand-include-all-switch'
import FieldFilterCard from './_modules/field-filter-card'
import FieldFilterSwitch from './_modules/field-filter-switch'

export const Route = createFileRoute('/(main)/main/settings/clash/runtime/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_runtime() } },
})

const FieldFilterSettings = () => {
  return (
    <ItemSection data-slot="field-filter-settings-container" className="gap-2">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_settings_field_filter_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <FieldFilterSwitch />
      </ItemGroup>

      <FieldFilterCard />
    </ItemSection>
  )
}

const ProfileProcessingSettings = () => {
  return (
    <ItemSection data-slot="profile-processing-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_runtime_processing_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <ExpandIncludeAllSwitch />

        <EnableBuiltinEnhancedSwitch />
      </ItemGroup>
    </ItemSection>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

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

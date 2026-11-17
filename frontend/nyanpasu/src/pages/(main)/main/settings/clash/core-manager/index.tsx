import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup } from '../../../_modules/settings-card'
import CoreManagerCard from './_modules/core-manager-card'

export const Route = createFileRoute(
  '/(main)/main/settings/clash/core-manager/',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_core_manager()}
      </PageTitle>

      <RequireCapability
        permission="system:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['system:write']}>
          <div className="space-y-4 px-4 pb-4">
            <div data-slot="core-manager-settings-container">
              <SettingsGroup>
                <CoreManagerCard />
              </SettingsGroup>
            </div>
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}

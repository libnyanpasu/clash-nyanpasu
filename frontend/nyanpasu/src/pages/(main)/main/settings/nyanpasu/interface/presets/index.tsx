import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import CustomCssPresets from './_modules/custom-css-presets'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface/presets/',
)({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_user_interface_custom_css_presets_group(),
    },
  },
})

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <RequireCapability
        permission="settings:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['settings:write']}>
          <div className="space-y-4 px-4 pb-4">
            <CustomCssPresets />
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}

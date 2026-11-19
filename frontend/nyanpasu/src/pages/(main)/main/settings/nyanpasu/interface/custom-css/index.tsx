import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import CustomCssItems from './_modules/custom-css-items'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface/custom-css/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_user_interface_custom_css_group() },
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
            <CustomCssItems />
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}

import { RequireCapability } from '@/components/auth/capabilities'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import AccessTokens from './_modules/access-tokens'
import BrowserSessions from './_modules/browser-sessions'
import ServerSettings from './_modules/server-settings'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/remote-access/',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_remote_access()}
      </PageTitle>

      <RequireCapability
        scope="remote_admin"
        fallback={
          <p className="text-on-surface-variant px-4 text-sm" role="status">
            {m.settings_remote_access_desktop_only()}
          </p>
        }
      >
        <div className="space-y-4 px-4 pb-4">
          <ServerSettings />

          <AccessTokens />

          <BrowserSessions />
        </div>
      </RequireCapability>
    </>
  )
}

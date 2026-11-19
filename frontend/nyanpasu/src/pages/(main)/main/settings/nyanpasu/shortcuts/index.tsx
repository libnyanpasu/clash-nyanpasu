import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup } from '../../../_modules/settings-card'
import HotkeyManager from './_modules/hotket-manager'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/shortcuts/',
)({
  component: RouteComponent,
  staticData: {
    crumb: { title: () => m.settings_nyanpasu_keyboard_shortcuts() },
  },
})

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <ReadOnlyScope permissions={['settings:write']}>
        <div className="space-y-4 px-4 pb-4">
          <div data-slot="keyboard-settings-container">
            <SettingsGroup>
              <HotkeyManager />
            </SettingsGroup>
          </div>
        </div>
      </ReadOnlyScope>
    </>
  )
}

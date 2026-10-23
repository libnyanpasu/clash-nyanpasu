import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup } from '../../../_modules/settings-card'
import HotkeyManager from './_modules/hotket-manager'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/shortcuts/',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_nyanpasu_keyboard_shortcuts()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="keyboard-settings-container">
          <SettingsGroup>
            <HotkeyManager />
          </SettingsGroup>
        </div>
      </div>
    </>
  )
}

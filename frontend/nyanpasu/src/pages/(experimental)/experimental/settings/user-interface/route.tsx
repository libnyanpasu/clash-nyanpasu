import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/user-interface',
)({
  component: RouteComponent,
  head: () => ({
    meta: [
      {
        title: m.settings_user_interface_title(),
      },
    ],
  }),
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_user_interface_title()}</SettingsTitle>
    </>
  )
}

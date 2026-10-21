import SystemUpdateAltRounded from '~icons/material-symbols/system-update-alt-rounded'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { SettingsCard } from '../../../_modules/settings-card'
import AboutCardHeader from './about-card-header'
import UpdateControls from './update-controls'

export default function UpdateStatusCard() {
  const { currentVersion } = useNyanpasuUpdate()

  return (
    <SettingsCard data-slot="about-update-card">
      <AboutCardHeader icon={SystemUpdateAltRounded}>
        {m.settings_label_about_version({ version: currentVersion })}
      </AboutCardHeader>

      <UpdateControls />
    </SettingsCard>
  )
}

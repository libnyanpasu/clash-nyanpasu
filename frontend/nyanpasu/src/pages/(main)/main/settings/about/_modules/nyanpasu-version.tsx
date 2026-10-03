import AnimatedLogo from '@/components/logo/animated-logo'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'
import UpdateControls from './update-controls'

export default function NyanpasuVersion({
  openChangelog = false,
}: {
  openChangelog?: boolean
}) {
  const { currentVersion } = useNyanpasuUpdate()

  return (
    <SettingsCard
      className="space-y-2 md:row-span-2"
      data-slot="about-version-card"
    >
      <SettingsCardContent className="items-center gap-4">
        <div className="p-4">
          <AnimatedLogo className="size-32" indeterminate />
        </div>

        <div className="truncate text-base font-bold">
          Clash Nyanpasu~(∠・ω&lt; )⌒☆
        </div>

        <div className="text-sm font-semibold">
          {m.settings_label_about_version({ version: currentVersion })}
        </div>
      </SettingsCardContent>

      <UpdateControls openChangelog={openChangelog} />
    </SettingsCard>
  )
}

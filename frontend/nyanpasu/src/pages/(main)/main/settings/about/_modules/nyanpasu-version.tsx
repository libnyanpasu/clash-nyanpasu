import AnimatedLogo from '@/components/logo/animated-logo'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function NyanpasuVersion() {
  const { currentVersion } = useNyanpasuUpdate()

  return (
    <SettingsCard data-slot="about-version-card">
      <SettingsCardContent className="p-3">
        <div
          className="flex flex-col items-center gap-3 px-5 py-6"
          data-slot="about-version-hero"
        >
          <AnimatedLogo className="size-32" indeterminate />

          <div className="text-on-surface max-w-full truncate text-xl font-semibold">
            Clash Nyanpasu~(∠・ω&lt; )⌒☆
          </div>

          <span className="bg-surface-variant text-on-surface-variant rounded-full px-3 py-1 text-sm font-medium">
            {m.settings_label_about_version({ version: currentVersion })}
          </span>
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}

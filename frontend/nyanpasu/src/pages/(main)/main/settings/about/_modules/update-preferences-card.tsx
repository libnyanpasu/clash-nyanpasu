import { SwitchItem } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { isBrowser } from '@nyanpasu/platform'
import { useSetting } from '@nyanpasu/query'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card'
import ReleaseChannelPreference from './release-channel-preference'

export default function UpdatePreferencesCard() {
  if (isBrowser()) {
    return (
      <SettingsCard data-slot="about-update-preferences-card">
        <SettingsCardHeader>
          <h2 className="text-base font-semibold">
            {m.settings_about_preferences_title()}
          </h2>
        </SettingsCardHeader>
        <SettingsCardContent className="pt-2">
          <p className="text-on-surface-variant text-sm">
            {m.settings_about_update_desktop_only()}
          </p>
        </SettingsCardContent>
      </SettingsCard>
    )
  }
  return <UpdatePreferencesControls />
}

function UpdatePreferencesControls() {
  const autoCheck = useSetting('enable_auto_check_update')
  const autoDownload = useSetting('enable_auto_download_update')

  const save = async (operation: Promise<unknown>) => {
    try {
      await operation
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <SettingsCard data-slot="about-update-preferences-card">
      <SettingsCardHeader>
        <h2 className="text-base font-semibold">
          {m.settings_about_preferences_title()}
        </h2>
      </SettingsCardHeader>
      <SettingsCardContent className="gap-2 pt-2">
        <SwitchItem
          className="rounded-[20px]"
          aria-label={m.settings_label_about_auto_check_updates()}
          checked={autoCheck.value ?? true}
          onCheckedChange={(checked) => save(autoCheck.upsert(checked))}
          loading={autoCheck.isPending}
          disabled={autoCheck.value == null || autoCheck.isPending}
        >
          <div className="flex flex-col gap-0.5">
            <span>{m.settings_label_about_auto_check_updates()}</span>
            <span className="text-on-surface-variant text-xs">
              {m.settings_about_auto_check_description()}
            </span>
          </div>
        </SwitchItem>
        <SwitchItem
          className="rounded-[20px]"
          aria-label={m.settings_about_auto_download_label()}
          checked={autoDownload.value ?? false}
          onCheckedChange={(checked) => save(autoDownload.upsert(checked))}
          loading={autoDownload.isPending}
          disabled={
            autoDownload.isPending ||
            autoCheck.isPending ||
            autoDownload.value == null ||
            autoCheck.value !== true
          }
        >
          <div className="flex flex-col gap-0.5">
            <span>{m.settings_about_auto_download_label()}</span>
            <span className="text-on-surface-variant text-xs">
              {m.settings_about_auto_download_description()}
            </span>
          </div>
        </SwitchItem>
        <ReleaseChannelPreference />
      </SettingsCardContent>
    </SettingsCard>
  )
}

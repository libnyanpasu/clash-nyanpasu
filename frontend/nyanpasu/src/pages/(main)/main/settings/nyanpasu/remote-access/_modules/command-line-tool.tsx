import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
import { useInstallCli, useIsAppImage } from '@nyanpasu/query'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
  SettingsLabel,
} from '../../../../_modules/settings-card'

/**
 * Links the bundled `nyanpasu-cli` onto `PATH`. Only the macOS app and the AppImage need it:
 * the Windows installer and the Linux packages already put the tool there.
 */
export default function CommandLineTool() {
  const isMacos = useMatchesPlatform({ surface: 'desktop', clientOs: 'macos' })
  const { data: isAppImage } = useIsAppImage()
  const install = useInstallCli()

  const handleCopyClick = useLockFn(async (command: string) => {
    try {
      await writeClipboardText(command)

      message(m.settings_remote_access_cli_copied(), {
        title: 'Success',
        kind: 'info',
      })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  if (!isMacos && !isAppImage) {
    return null
  }

  return (
    <div data-slot="remote-access-cli-container">
      <SettingsLabel>{m.settings_remote_access_cli_label()}</SettingsLabel>

      <SettingsCard>
        <SettingsCardContent className="gap-4">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_remote_access_cli_title()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_remote_access_cli_description()}
            </ItemLabelDescription>

            {isAppImage && (
              <ItemLabelDescription data-slot="remote-access-cli-appimage">
                {m.settings_remote_access_cli_appimage()}
              </ItemLabelDescription>
            )}
          </ItemLabel>

          {install.data && (
            <div
              className="flex flex-col gap-2 text-sm"
              data-slot="remote-access-cli-result"
            >
              <p className="select-text">
                {m.settings_remote_access_cli_installed({
                  link: install.data.link,
                })}
              </p>

              {!install.data.on_path && (
                <p
                  className="text-on-surface-variant select-text"
                  data-slot="remote-access-cli-not-on-path"
                  role="note"
                >
                  {m.settings_remote_access_cli_not_on_path({
                    link: install.data.link,
                  })}
                </p>
              )}

              <p className="text-on-surface-variant">
                {m.settings_remote_access_cli_system_command()}
              </p>

              <div className="flex items-center gap-2">
                <code
                  className="bg-surface-container-high grow rounded-xl p-3 wrap-anywhere select-text"
                  data-slot="remote-access-cli-system-command"
                >
                  {install.data.system_command}
                </code>

                <Button
                  variant="flat"
                  aria-label={m.common_copy()}
                  onClick={() => handleCopyClick(install.data.system_command)}
                >
                  <ContentCopyRounded />
                </Button>
              </div>
            </div>
          )}

          {install.error && (
            <p
              className="text-error text-sm"
              data-slot="remote-access-cli-error"
              role="alert"
            >
              {m.settings_remote_access_cli_failed({
                error: formatError(install.error),
              })}
            </p>
          )}
        </SettingsCardContent>

        <SettingsCardFooter>
          <Button
            variant="flat"
            loading={install.isPending}
            onClick={() => install.mutate()}
          >
            {m.settings_remote_access_cli_install()}
          </Button>
        </SettingsCardFooter>
      </SettingsCard>
    </div>
  )
}

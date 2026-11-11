import ContentCopyOutlineRounded from '~icons/material-symbols/content-copy-outline-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Modal, ModalTrigger } from '@nyanpasu/ui/modal'
import { Switch } from '@nyanpasu/ui/switch'
import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import useCustomCss, { reportCustomCssError } from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { usePlatform } from '@nyanpasu/platform/react'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import CustomCssNameDialog from './custom-css-name-dialog'

/**
 * The stylesheets bundled with the app. They cannot be edited; duplicate one
 * to start from it. Renders nothing when the app ships none.
 */
export default function CustomCssPresets() {
  const customCss = useCustomCss()
  const { clientOs } = usePlatform()

  const presets = CUSTOM_CSS_PRESETS.filter(
    (preset) => preset.platform !== 'macos' || clientOs === 'macos',
  )

  if (presets.length === 0) {
    return null
  }

  return (
    <div data-slot="custom-css-presets-container">
      <SettingsLabel>
        {m.settings_user_interface_custom_css_presets_group()}
      </SettingsLabel>

      <SettingsGroup>
        {presets.map((preset) => (
          <Modal key={preset.id}>
            <SettingsCard data-slot="custom-css-preset">
              <SettingsCardContent>
                <ItemContainer>
                  <ItemLabel>
                    <ItemLabelText>{preset.name()}</ItemLabelText>

                    <ItemLabelDescription>
                      {preset.description()}
                    </ItemLabelDescription>
                  </ItemLabel>

                  <div className="flex shrink-0 items-center gap-2">
                    <ModalTrigger asChild>
                      <Button
                        type="button"
                        icon
                        aria-label={m.settings_user_interface_custom_css_preset_duplicate()}
                        title={m.settings_user_interface_custom_css_preset_duplicate()}
                        className="text-on-surface-variant size-9"
                        data-slot="custom-css-preset-duplicate"
                      >
                        <ContentCopyOutlineRounded
                          className="size-5"
                          aria-hidden
                        />
                      </Button>
                    </ModalTrigger>

                    <Switch
                      aria-label={preset.name()}
                      checked={customCss.state.enabledPresets.includes(
                        preset.id,
                      )}
                      onCheckedChange={(enabled) =>
                        customCss
                          .setPreset(preset.id, enabled)
                          .catch(reportCustomCssError)
                      }
                    />
                  </div>
                </ItemContainer>
              </SettingsCardContent>
            </SettingsCard>
            <CustomCssNameDialog
              title={m.settings_user_interface_custom_css_duplicate_title()}
              initialName={m.settings_user_interface_custom_css_preset_copy_name(
                { name: preset.name() },
              )}
              onSubmit={async (name) => {
                await customCss.duplicatePreset(preset.id, name)
              }}
            />
          </Modal>
        ))}
      </SettingsGroup>
    </div>
  )
}

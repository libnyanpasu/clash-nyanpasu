import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@/components/ui/button'
import useCustomCss from '@/hooks/use-custom-css'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function CustomCssCard() {
  const { value: css } = useCustomCss()

  const charCount = css?.length ?? 0
  const isLarge = charCount > 100_000

  const handleOpen = useLockFn(async () => {
    await commands.createEditorWindow('css-editor', null)
  })

  return (
    <SettingsCard data-slot="custom-css-card">
      <SettingsCardContent data-slot="custom-css-card-content" asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleOpen}
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>
                {m.settings_user_interface_custom_css_label()}
              </ItemLabelText>

              <ItemLabelDescription>
                {charCount > 0
                  ? m.settings_user_interface_custom_css_chars({
                      count: charCount,
                    })
                  : m.settings_user_interface_custom_css_empty()}
                {isLarge && (
                  <span className="text-warning ml-2">
                    {m.settings_user_interface_custom_css_large_warning()}
                  </span>
                )}
              </ItemLabelDescription>
            </ItemLabel>

            <ArrowForwardIosRounded />
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

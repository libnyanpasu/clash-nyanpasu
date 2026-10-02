import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import useCustomCss from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
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
    await rpc.createEditorWindow('css-editor', null)
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

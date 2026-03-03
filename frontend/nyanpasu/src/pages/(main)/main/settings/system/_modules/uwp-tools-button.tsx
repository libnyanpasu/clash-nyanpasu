import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@/components/ui/button'
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

export default function UwpToolsButton() {
  const handleOpenUwpTools = useLockFn(async () => {
    await commands.invokeUwpTool()
  })

  return (
    <SettingsCard data-slot="uwp-tools-button-card">
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleOpenUwpTools}
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>
                {m.settings_system_proxy_uwp_tools_label()}
              </ItemLabelText>

              <ItemLabelDescription>
                {m.settings_system_proxy_uwp_tools_description()}
              </ItemLabelDescription>
            </ItemLabel>

            <div>
              <ArrowForwardIosRounded />
            </div>
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

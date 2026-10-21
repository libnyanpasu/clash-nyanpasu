import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card'
import {
  openIssueReport,
  openRepository,
  openWiki,
} from '../../../../_modules/help-links'

function HelpLink({
  label,
  description,
  onOpen,
}: {
  label: string
  description: string
  onOpen: () => Promise<unknown>
}) {
  const handleClick = useLockFn(onOpen)

  return (
    <SettingsCard>
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleClick}
          data-slot="about-help-link"
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>{label}</ItemLabelText>

              <ItemLabelDescription>{description}</ItemLabelDescription>
            </ItemLabel>

            <OpenInNewRounded className="text-on-surface-variant size-5 shrink-0" />
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

export default function HelpLinks() {
  return (
    <div data-slot="about-help-links">
      <SettingsLabel>{m.settings_about_help_label()}</SettingsLabel>

      <SettingsGroup>
        <HelpLink
          label={m.header_help_action_wiki()}
          description={m.settings_about_help_wiki_description()}
          onOpen={openWiki}
        />

        <HelpLink
          label={m.header_help_action_issues()}
          description={m.settings_about_help_issues_description()}
          onOpen={openIssueReport}
        />

        <HelpLink
          label="GitHub"
          description={m.settings_about_help_github_description()}
          onOpen={openRepository}
        />
      </SettingsGroup>
    </div>
  )
}

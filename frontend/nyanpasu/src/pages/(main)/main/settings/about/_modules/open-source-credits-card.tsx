import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import LicenseOutlineRounded from '~icons/material-symbols/license-outline-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'
import AboutCardHeader from './about-card-header'

export default function OpenSourceCreditsCard() {
  return (
    <SettingsCard data-slot="about-credits-card">
      <AboutCardHeader icon={LicenseOutlineRounded}>
        {m.settings_about_credits_title()}
      </AboutCardHeader>

      <SettingsCardContent className="pt-2">
        <p className="text-on-surface-variant text-sm">
          {m.settings_about_credits_description()}
        </p>
      </SettingsCardContent>

      <SettingsCardFooter className="pt-0" data-slot="about-credits-actions">
        <Button
          variant="stroked"
          className="flex w-full items-center justify-center gap-1"
          asChild
        >
          <Link to="/main/settings/about/credits">
            {m.settings_about_credits_view()}
            <ChevronRightRounded className="size-5" />
          </Link>
        </Button>
      </SettingsCardFooter>
    </SettingsCard>
  )
}

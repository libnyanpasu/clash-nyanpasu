import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Link, type LinkProps } from '@tanstack/react-router'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

/**
 * A settings section's entry row: one link to the page that owns the section's
 * controls, so the row itself stays a single interactive element.
 */
export function SettingsLinkRow({
  to,
  params,
  search,
  title,
  description,
  'data-slot': slot,
}: {
  to: LinkProps['to']
  params?: LinkProps['params']
  search?: LinkProps['search']
  title: string
  description: string
  /** The entry's own styling slot; the row root keeps `settings-link-row`. */
  'data-slot'?: string
}) {
  return (
    <SettingsCard data-slot="settings-link-row">
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          asChild
        >
          <Link to={to} params={params} search={search} data-slot={slot}>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>{title}</ItemLabelText>

                <ItemLabelDescription>{description}</ItemLabelDescription>
              </ItemLabel>

              <ArrowForwardIosRounded />
            </ItemContainer>
          </Link>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

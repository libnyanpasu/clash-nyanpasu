import z from 'zod'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../_modules/settings-card'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import NyanpasuVersion from './_modules/nyanpasu-version'

export enum Action {
  NEED_UPDATE = 'need-update',
}

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
  validateSearch: z.object({
    action: z.enum(Action).optional().nullable(),
  }),
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_label_about()}</SettingsTitle>

      <SettingsCard>
        <SettingsCardContent>
          <div className="grid gap-2 sm:grid-cols-2">
            <NyanpasuVersion />
          </div>
        </SettingsCardContent>
      </SettingsCard>
    </>
  )
}

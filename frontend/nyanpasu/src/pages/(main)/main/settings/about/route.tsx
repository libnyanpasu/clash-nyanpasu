import z from 'zod'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsTitle } from '../_modules/settings-title'
import NyanpasuVersion from './_modules/nyanpasu-version'
import UpdatePreferencesCard from './_modules/update-preferences-card'
import UpdateSourceSelector from './_modules/update-source-selector'

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
  const { action } = Route.useSearch()

  return (
    <>
      <SettingsTitle>{m.settings_label_about()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <div className="grid grid-cols-1 items-start gap-4 md:grid-cols-2">
          <NyanpasuVersion openChangelog={action === Action.NEED_UPDATE} />
          <UpdateSourceSelector />
          <UpdatePreferencesCard />
        </div>
      </div>
    </>
  )
}

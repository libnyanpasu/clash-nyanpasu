import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import NyanpasuVersion from './_modules/nyanpasu-version'
import UpdatePreferencesCard from './_modules/update-preferences-card'
import UpdateSourceSelector from './_modules/update-source-selector'

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_about()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div className="grid grid-cols-1 items-start gap-4 md:grid-cols-2">
          <NyanpasuVersion />
          <UpdateSourceSelector />
          <UpdatePreferencesCard />
        </div>
      </div>
    </>
  )
}

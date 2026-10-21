import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import HelpLinks from './_modules/help-links'
import NyanpasuVersion from './_modules/nyanpasu-version'

export const Route = createFileRoute('/(main)/main/settings/about/')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_about()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <NyanpasuVersion />

        <HelpLinks />
      </div>
    </>
  )
}

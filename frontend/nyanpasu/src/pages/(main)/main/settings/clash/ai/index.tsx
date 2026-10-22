import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLabel } from '../../_modules/settings-card'
import MeowCliSkillCard from './_modules/meow-cli-skill-card'

export const Route = createFileRoute('/(main)/main/settings/clash/ai/')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_ai()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="meow-cli-skill-container">
          <SettingsLabel>{m.settings_ai_skill_label()}</SettingsLabel>

          <MeowCliSkillCard />
        </div>
      </div>
    </>
  )
}

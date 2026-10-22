import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useCoreAgentSkills, useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLabel } from '../../_modules/settings-card'
import CoreAgentSkillCard from './_modules/core-agent-skill-card'

export const Route = createFileRoute('/(main)/main/settings/clash/ai/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { value: currentCore } = useSetting('core')
  const { data: skills } = useCoreAgentSkills(currentCore)

  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_ai()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div className="space-y-2" data-slot="core-agent-skill-container">
          <SettingsLabel>{m.settings_ai_skill_label()}</SettingsLabel>

          {skills?.map((skill) => (
            <CoreAgentSkillCard key={skill.name} skill={skill} />
          ))}
        </div>
      </div>
    </>
  )
}

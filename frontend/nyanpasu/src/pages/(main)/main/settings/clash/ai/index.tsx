import { CircularProgress } from '@nyanpasu/ui/progress'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useCoreAgentSkills, useSetting } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsLabel,
} from '../../_modules/settings-card'
import CoreAgentSkillCard from './_modules/core-agent-skill-card'

export const Route = createFileRoute('/(main)/main/settings/clash/ai/')({
  component: RouteComponent,
})

function RouteComponent() {
  const { value: currentCore } = useSetting('core')
  const { value: serviceMode } = useSetting('enable_service_mode')
  const { data: skills, error, isPending } = useCoreAgentSkills(currentCore)

  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_ai()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div className="space-y-2" data-slot="core-agent-skill-container">
          <SettingsLabel>{m.settings_ai_skill_label()}</SettingsLabel>

          {isPending ? (
            <div
              className="text-on-surface-variant flex items-center gap-2 px-2 text-sm"
              data-slot="core-agent-skill-loading"
            >
              <CircularProgress className="size-4" indeterminate />
              {m.settings_ai_skill_loading()}
            </div>
          ) : error ? (
            <p className="text-error px-2 text-sm" role="alert">
              {m.settings_ai_skill_failed({ error: formatError(error) })}
            </p>
          ) : skills.length > 0 ? (
            <>
              {/* The core then runs as root or SYSTEM, whose controller file the user cannot read. */}
              {serviceMode && (
                <p
                  className="text-on-surface-variant px-2 text-sm"
                  data-slot="core-agent-skill-service-mode"
                >
                  {m.settings_ai_skill_service_mode()}
                </p>
              )}

              {skills.map((skill) => (
                <CoreAgentSkillCard key={skill.name} skill={skill} />
              ))}
            </>
          ) : (
            <SettingsCard data-slot="core-agent-skill-none">
              <SettingsCardContent>
                <p className="text-on-surface-variant text-sm">
                  {m.settings_ai_skill_none()}
                </p>
              </SettingsCardContent>
            </SettingsCard>
          )}
        </div>
      </div>
    </>
  )
}

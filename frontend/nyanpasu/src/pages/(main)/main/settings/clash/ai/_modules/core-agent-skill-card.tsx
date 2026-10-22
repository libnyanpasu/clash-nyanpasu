import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import type { CoreAgentSkill } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import {
  ItemLabelDescription,
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../../../_modules/settings-card'

export default function CoreAgentSkillCard({
  skill,
}: {
  skill: CoreAgentSkill
}) {
  const prompt = m.settings_ai_skill_prompt({ path: skill.dir })

  const handleCopyClick = useLockFn(async () => {
    try {
      await writeClipboardText(prompt)

      message(m.settings_ai_skill_prompt_copied(), {
        title: 'Success',
        kind: 'info',
      })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  // Only a core update installs the skill.
  if (!skill.installed) {
    return (
      <SettingsCard data-slot="core-agent-skill-card">
        <SettingsCardContent>
          <ItemLabelDescription>
            {m.settings_ai_skill_not_installed()}
          </ItemLabelDescription>
        </SettingsCardContent>

        <SettingsCardFooter>
          <Button variant="flat" asChild>
            <Link
              className="flex items-center"
              to="/main/settings/clash/core-manager"
            >
              {m.settings_label_core_manager()}
            </Link>
          </Button>
        </SettingsCardFooter>
      </SettingsCard>
    )
  }

  return (
    <SettingsCard data-slot="core-agent-skill-card">
      <SettingsCardContent className="gap-4">
        <ItemLabelDescription>
          {m.settings_ai_skill_description()}
        </ItemLabelDescription>

        <p
          className="bg-surface-variant/30 rounded-xl p-3 text-sm leading-6 wrap-anywhere select-text"
          data-slot="core-agent-skill-prompt"
        >
          {prompt}
        </p>
      </SettingsCardContent>

      <SettingsCardFooter>
        <Button
          variant="flat"
          className="flex items-center gap-2"
          onClick={handleCopyClick}
        >
          <ContentCopyRounded />
          <span>{m.common_copy()}</span>
        </Button>
      </SettingsCardFooter>
    </SettingsCard>
  )
}

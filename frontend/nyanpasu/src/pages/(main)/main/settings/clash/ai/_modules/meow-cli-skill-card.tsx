import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useMeowCliSkill } from '@nyanpasu/query'
import { Link } from '@tanstack/react-router'
import {
  ItemLabelDescription,
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../../_modules/settings-card'

export default function MeowCliSkillCard() {
  const { data } = useMeowCliSkill()

  const prompt = data ? m.settings_ai_skill_prompt({ path: data.dir }) : ''

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

  if (!data) {
    return null
  }

  // Only a Meow core update installs meow-cli and its skill.
  if (!data.installed) {
    return (
      <SettingsCard data-slot="meow-cli-skill-card">
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
    <SettingsCard data-slot="meow-cli-skill-card">
      <SettingsCardContent className="gap-4">
        <ItemLabelDescription>
          {m.settings_ai_skill_description()}
        </ItemLabelDescription>

        <p
          className="bg-surface-variant/30 rounded-xl p-3 text-sm leading-6 wrap-anywhere select-text"
          data-slot="meow-cli-skill-prompt"
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

import MdiTrayFull from '~icons/mdi/tray-full'
import { useLockFn } from 'ahooks'
import React, { lazy } from 'react'
import { useTranslation } from 'react-i18next'
import HotkeyDialog from '@/components/setting/modules/hotkey-dialog'
import TrayIconDialog from '@/components/setting/modules/tray-icon-dialog'
import { formatEnvInfos } from '@/utils'
import { Feedback, GitHub, Keyboard } from '@mui/icons-material'
import { IconButton } from '@mui/material'
import { collectEnvs, openThat } from '@nyanpasu/interface'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/settings')({
  component: SettingPage,
})

function SettingPage() {
  const { t } = useTranslation()

  const Component = lazy(() => import('@/components/setting/setting-page'))

  const GithubIcon = () => {
    const toGithubRepo = useLockFn(() => {
      return openThat('https://github.com/libnyanpasu/clash-nyanpasu')
    })

    return (
      <IconButton
        color="inherit"
        title="@libnyanpasu/clash-nyanpasu"
        onClick={toGithubRepo}
      >
        <GitHub fontSize="inherit" />
      </IconButton>
    )
  }

  const FeedbackIcon = () => {
    const toFeedback = useLockFn(async () => {
      const envs = await collectEnvs()
      const formattedEnv = encodeURIComponent(
        formatEnvInfos(envs)
          .split('\n')
          .map((v) => `> ${v}`)
          .join('\n'),
      )
      return openThat(
        'https://github.com/libnyanpasu/clash-nyanpasu/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=' +
          formattedEnv,
      )
    })
    return (
      <IconButton color="inherit" title={t('Feedback')} onClick={toFeedback}>
        <Feedback fontSize="inherit" />
      </IconButton>
    )
  }

  // FIXME: it should move to a proper place
  const HotkeyButton = () => {
    const [open, setOpen] = React.useState(false)
    return (
      <>
        <HotkeyDialog open={open} onClose={() => setOpen(false)} />
        <IconButton
          color="inherit"
          title={t('Hotkeys')}
          onClick={() => setOpen(true)}
        >
          <Keyboard fontSize="inherit" />
        </IconButton>
      </>
    )
  }

  // FIXME: it should move to a proper place
  const TrayIconButton = () => {
    const [open, setOpen] = React.useState(false)
    return (
      <>
        <TrayIconDialog open={open} onClose={() => setOpen(false)} />
        <IconButton
          color="inherit"
          title={t('Tray Icons')}
          onClick={() => setOpen(true)}
        >
          <MdiTrayFull fontSize="inherit" />
        </IconButton>
      </>
    )
  }

  return (
    <BasePage
      title={t('Settings')}
      header={
        <div className="flex gap-1">
          <TrayIconButton />
          <HotkeyButton />
          <FeedbackIcon />
          <GithubIcon />
        </div>
      }
      sectionStyle={{
        paddingRight: 0,
      }}
    >
      <Component />
    </BasePage>
  )
}

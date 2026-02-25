import { startCase } from 'lodash-es'
import { useEffect, useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { OS } from '@/consts'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { getShikiSingleton } from '@/utils/shiki'
import {
  commands,
  useCoreDir,
  useServicePrompt,
  useSystemService,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const SystemServiceCtrlItem = ({
  name,
  value,
}: {
  name: string
  value?: string
}) => {
  return (
    <div className="flex w-full leading-8" data-slot="system-service-ctrl-item">
      <div
        className="w-32 capitalize"
        data-slot="system-service-ctrl-item-name"
      >
        {name}:
      </div>

      <div
        className="text-warp flex-1 break-all"
        data-slot="system-service-ctrl-item-value"
      >
        {value ?? '-'}
      </div>
    </div>
  )
}

const ServiceDetailButton = () => {
  const { query } = useSystemService()

  return (
    <Modal>
      <ModalTrigger asChild>
        <Button data-slot="system-service-detail-button">
          {m.settings_system_proxy_system_service_ctrl_detail()}
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>
              {m.settings_system_proxy_system_service_ctrl_detail()}
            </ModalTitle>
          </CardHeader>

          <CardContent>
            <pre className="overflow-auto font-mono select-text">
              {JSON.stringify(query.data, null, 2)}
            </pre>
          </CardContent>

          <CardFooter>
            <ModalClose>{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

const ServiceInstallButton = () => {
  const { upsert } = useSystemService()

  const handleInstallClick = useLockFn(async () => {
    try {
      await upsert.mutateAsync('install')
      await commands.restartSidecar()
    } catch (e) {
      const errorMessage = `${m.settings_system_proxy_system_service_ctrl_failed_install()}: ${formatError(e)}`

      message(errorMessage, {
        kind: 'error',
      })
      // // If the installation fails, prompt the user to manually install the service
      // promptDialog.show(
      //   query.data?.status === 'not_installed' ? 'install' : 'uninstall',
      // )
    }
  })

  return (
    <Button
      variant="flat"
      onClick={handleInstallClick}
      loading={upsert.isPending}
    >
      {m.settings_system_proxy_system_service_ctrl_install()}
    </Button>
  )
}

const ServiceUninstallButton = () => {
  const { upsert } = useSystemService()

  const handleUninstallClick = useLockFn(async () => {
    await upsert.mutateAsync('uninstall')
  })

  return (
    <Button onClick={handleUninstallClick} loading={upsert.isPending}>
      {m.settings_system_proxy_system_service_ctrl_uninstall()}
    </Button>
  )
}
// {
//   operation: 'uninstall' | 'install' | 'start' | 'stop' | null
// }
const ServicePromptButton = () => {
  const {
    query: { data: systemService },
  } = useSystemService()

  const { data: serviceInstallPrompt } = useServicePrompt()

  const { data: coreDir } = useCoreDir()

  const [codes, setCodes] = useState<string | null>(null)

  const userOperationCommands = useMemo(() => {
    if (systemService?.status === 'not_installed' && serviceInstallPrompt) {
      return `cd "${coreDir}"\n${serviceInstallPrompt}`
    } else if (systemService?.status) {
      const operation = systemService?.status === 'running' ? 'stop' : 'start'

      return `cd "${coreDir}"\n${OS !== 'windows' ? 'sudo ' : ''}./nyanpasu-service ${operation}`
    }
    return ''
  }, [systemService?.status, serviceInstallPrompt, coreDir])

  useEffect(() => {
    const handleGenerateCodes = async () => {
      const shiki = await getShikiSingleton()
      const code = shiki.codeToHtml(userOperationCommands, {
        lang: 'shell',
        themes: {
          dark: 'nord',
          light: 'min-light',
        },
      })

      setCodes(code)
    }

    handleGenerateCodes()
  }, [userOperationCommands])

  const handleCopyToClipboard = useLockFn(async () => {
    if (!userOperationCommands) {
      return
    }

    await writeText(userOperationCommands)
  })

  return (
    <Modal>
      <ModalTrigger asChild>
        <Button variant="flat">
          {m.settings_system_proxy_system_service_ctrl_prompt()}
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="max-w-3xl min-w-96">
          <CardHeader>
            <ModalTitle>
              {m.settings_system_proxy_system_service_ctrl_manual_prompt()}
            </ModalTitle>
          </CardHeader>

          <CardContent>
            <p className="leading-6">
              {m.settings_system_proxy_system_service_ctrl_manual_operation_prompt()}
            </p>

            {codes && (
              <div
                className={cn(
                  'overflow-clip rounded select-text',
                  '[&>pre]:overflow-auto [&>pre]:p-2',
                  '[&>pre]:bg-surface-variant! dark:[&>pre]:bg-black!',
                )}
                dangerouslySetInnerHTML={{
                  __html: codes,
                }}
              />
            )}
          </CardContent>

          <CardFooter className="gap-2">
            <Button variant="flat" onClick={handleCopyToClipboard}>
              {m.common_copy()}
            </Button>

            <ModalClose>{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

const ServiceControlButtons = () => {
  const { query, upsert } = useSystemService()

  const handleToggleClick = useLockFn(async () => {
    await upsert.mutateAsync(
      query.data?.status === 'running' ? 'stop' : 'start',
    )
  })

  return (
    <Button
      variant="flat"
      onClick={handleToggleClick}
      loading={upsert.isPending}
    >
      {query.data?.status === 'running'
        ? m.settings_system_proxy_system_service_ctrl_stop()
        : m.settings_system_proxy_system_service_ctrl_start()}
    </Button>
  )
}

export default function SystemServiceCtrl() {
  const { query } = useSystemService()

  const isInstalled = query.data?.status !== 'not_installed'

  return (
    <SettingsCard data-slot="system-service-ctrl-card">
      <SettingsCardContent
        data-slot="system-service-ctrl-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <Card>
          <CardHeader>
            {m.settings_system_proxy_system_service_ctrl_label()}
          </CardHeader>

          <CardContent className="gap-1 select-text">
            <SystemServiceCtrlItem
              name="Service Name"
              value={query.data?.name}
            />

            <SystemServiceCtrlItem
              name="Server Version"
              value={query.data?.server?.version}
            />

            <SystemServiceCtrlItem
              name="Service Status"
              value={startCase(query.data?.status)}
            />
          </CardContent>

          <CardFooter className="flex-wrap-reverse gap-2">
            {isInstalled ? (
              <>
                <ServiceControlButtons />

                <ServiceUninstallButton />
              </>
            ) : (
              <ServiceInstallButton />
            )}

            <ServiceDetailButton />

            <div className="flex-1" />

            <ServicePromptButton />
          </CardFooter>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}

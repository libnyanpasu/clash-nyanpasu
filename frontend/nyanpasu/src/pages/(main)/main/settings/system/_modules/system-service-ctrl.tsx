import { startCase } from 'es-toolkit/compat'
import { AnimatePresence } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { OS } from '@nyanpasu/platform'
import { useCoreDir, useServicePrompt, useSystemService } from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import { cn } from '@nyanpasu/utils'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'

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

const ServiceCompatWarning = () => {
  const { query } = useSystemService()

  const compat = query.data?.compat

  const warning =
    compat?.kind === 'incompatible'
      ? m.settings_system_proxy_system_service_ctrl_incompatible({
          version: compat.server_version,
          required: compat.required_major,
        })
      : compat?.kind === 'unparsable'
        ? m.settings_system_proxy_system_service_ctrl_unparsable({
            version: compat.server_version,
          })
        : null

  return (
    <AnimatePresence initial={false}>
      {warning && (
        <SettingsCardAnimatedItem
          className="text-error"
          data-slot="system-service-compat-warning"
        >
          {warning}
        </SettingsCardAnimatedItem>
      )}
    </AnimatePresence>
  )
}

const ServiceInstallButton = () => {
  const { upsert } = useSystemService()

  const handleInstallClick = useLockFn(async () => {
    try {
      await upsert.mutateAsync('install')
      unwrapResult(await rpc.restartSidecar())
    } catch (e) {
      const errorMessage = `${m.settings_system_proxy_system_service_ctrl_failed_install()}: ${formatError(e)}`

      message(errorMessage, {
        kind: 'error',
        error: e,
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
    try {
      await upsert.mutateAsync('uninstall')
    } catch (e) {
      message(
        `${m.settings_system_proxy_system_service_ctrl_failed_uninstall()}: ${formatError(e)}`,
        {
          kind: 'error',
          error: e,
        },
      )
    }
  })

  return (
    <Button onClick={handleUninstallClick} loading={upsert.isPending}>
      {m.settings_system_proxy_system_service_ctrl_uninstall()}
    </Button>
  )
}
// Rendered in the open prompt only: highlighting loads shiki and its WASM
// engine.
const ServicePromptCode = ({ code }: { code: string }) => {
  const [highlighted, setHighlighted] = useState<{
    code: string
    html: string
  }>()

  useEffect(() => {
    let cancelled = false
    import('@/utils/shiki')
      .then(({ getShikiSingleton }) => getShikiSingleton())
      .then((shiki) => {
        if (cancelled) return
        setHighlighted({
          code,
          html: shiki.codeToHtml(code, {
            lang: 'shell',
            themes: {
              dark: 'nord',
              light: 'min-light',
            },
          }),
        })
      })
    return () => {
      cancelled = true
    }
  }, [code])

  const className = cn(
    'overflow-clip rounded select-text',
    '[&>pre]:overflow-auto [&>pre]:p-2',
    '[&>pre]:bg-surface-variant! dark:[&>pre]:bg-black!',
  )

  if (highlighted?.code !== code) {
    return (
      <div className={className}>
        <pre>
          <code>{code}</code>
        </pre>
      </div>
    )
  }

  return (
    <div
      className={className}
      dangerouslySetInnerHTML={{
        __html: highlighted.html,
      }}
    />
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

  const userOperationCommands = useMemo(() => {
    if (systemService?.status === 'not_installed' && serviceInstallPrompt) {
      return `cd "${coreDir}"\n${serviceInstallPrompt}`
    } else if (systemService?.status) {
      const operation = systemService?.status === 'running' ? 'stop' : 'start'

      return `cd "${coreDir}"\n${OS !== 'windows' ? 'sudo ' : ''}./nyanpasu-service ${operation}`
    }
    return ''
  }, [systemService?.status, serviceInstallPrompt, coreDir])

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

            <ServicePromptCode code={userOperationCommands} />
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
    const operation = query.data?.status === 'running' ? 'stop' : 'start'

    try {
      await upsert.mutateAsync(operation)
    } catch (e) {
      const errorTitle =
        operation === 'stop'
          ? m.settings_system_proxy_system_service_ctrl_failed_stop()
          : m.settings_system_proxy_system_service_ctrl_failed_start()

      message(`${errorTitle}: ${formatError(e)}`, { kind: 'error', error: e })
    }
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
    <SettingsCard>
      <SettingsCardContent className="gap-2 py-4">
        <SystemServiceCtrlItem name="Service Name" value={query.data?.name} />

        <SystemServiceCtrlItem
          name="Server Version"
          value={query.data?.server?.version}
        />

        <SystemServiceCtrlItem
          name="Service Status"
          value={startCase(query.data?.status)}
        />

        <ServiceCompatWarning />
      </SettingsCardContent>

      <SettingsCardFooter className="flex-wrap-reverse gap-2">
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
      </SettingsCardFooter>
    </SettingsCard>
  )
}

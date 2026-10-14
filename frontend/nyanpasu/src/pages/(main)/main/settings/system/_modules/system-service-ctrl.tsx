import { startCase } from 'es-toolkit/compat'
import { AnimatePresence } from 'motion/react'
import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { isWindows } from '@nyanpasu/platform'
import { useCoreDir, useServicePrompt, useSystemService } from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import type {
  CoreControllerInfo,
  CoreState,
  CoreType,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
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

const formatCoreType = (type?: CoreType | null) =>
  type == null ? undefined : typeof type === 'string' ? type : type.clash

const formatCoreState = (state?: CoreState) => {
  if (state == null || state === 'Running') {
    return state
  }

  return state.Stopped ? `Stopped (${state.Stopped})` : 'Stopped'
}

const formatCoreController = (controller?: CoreControllerInfo | null) =>
  controller ? Object.values(controller)[0] : undefined

const ServiceDetailSection = ({
  title,
  items,
}: {
  title: string
  items: { name: string; value?: string | number | null }[]
}) => {
  return (
    <section
      className="col-span-full grid grid-cols-subgrid gap-y-2"
      data-slot="service-detail-section"
    >
      <h3
        className="text-primary col-span-full text-sm font-medium"
        data-slot="service-detail-section-title"
      >
        {title}
      </h3>

      <dl
        className="col-span-full grid grid-cols-subgrid gap-y-1 text-sm leading-6"
        data-slot="service-detail-section-items"
      >
        {items.map(({ name, value }) => (
          <Fragment key={name}>
            <dt className="text-on-surface-variant">{name}</dt>

            <dd className="font-mono break-all select-text">{value ?? '-'}</dd>
          </Fragment>
        ))}
      </dl>
    </section>
  )
}

// Rendered only while expanded: highlighting loads shiki and its WASM engine.
const ServiceDetailRawJson = ({ code }: { code: string }) => {
  const [highlighted, setHighlighted] = useState<{
    code: string
    html: string
  }>()

  const ref = useRef<HTMLDivElement>(null)

  // The block opens below the fields, out of view in a short window.
  useEffect(() => {
    ref.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }, [])

  useEffect(() => {
    let cancelled = false
    import('@/utils/shiki')
      .then(({ highlightJson }) => highlightJson(code))
      .then((html) => {
        if (!cancelled) setHighlighted({ code, html })
      })
    return () => {
      cancelled = true
    }
  }, [code])

  return (
    <div
      ref={ref}
      className={cn(
        'bg-surface-variant/30 rounded-xl p-3 text-xs select-text',
        '[&_.shiki]:bg-transparent! [&_.shiki_span]:bg-transparent!',
        'dark:[&_.shiki]:text-(--shiki-dark)! dark:[&_.shiki_span]:text-(--shiki-dark)!',
        '[&_pre]:font-mono [&_pre]:leading-relaxed [&_pre]:wrap-anywhere [&_pre]:whitespace-pre-wrap',
      )}
      data-slot="service-detail-raw-json"
    >
      {highlighted?.code === code ? (
        <div dangerouslySetInnerHTML={{ __html: highlighted.html }} />
      ) : (
        <pre>
          <code>{code}</code>
        </pre>
      )}
    </div>
  )
}

const ServiceDetailButton = () => {
  const { query } = useSystemService()

  const [showRawJson, setShowRawJson] = useState(false)

  const data = query.data
  const server = data?.server
  const core = server?.core_infos

  const rawJson = useMemo(() => JSON.stringify(data, null, 2), [data])

  return (
    <Modal onOpenChange={(open) => !open && setShowRawJson(false)}>
      <ModalTrigger asChild>
        <Button data-slot="system-service-detail-button">
          {m.settings_system_proxy_system_service_ctrl_detail()}
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card
          className="flex max-h-[90dvh] w-[min(48rem,calc(100vw-2rem))] flex-col"
          data-slot="service-detail"
        >
          <CardHeader className="shrink-0">
            <ModalTitle>
              {m.settings_system_proxy_system_service_ctrl_detail()}
            </ModalTitle>
          </CardHeader>

          <ScrollArea className="min-h-0 flex-1">
            <CardContent>
              {/* Sections share one label column through subgrid. */}
              <div
                className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-4"
                data-slot="service-detail-sections"
              >
                <ServiceDetailSection
                  title={m.settings_system_proxy_system_service_ctrl_detail_service()}
                  items={[
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_name(),
                      value: data?.name,
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_status(),
                      value: startCase(data?.status),
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_phase(),
                      value: startCase(data?.phase),
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_service_version(),
                      value: data?.version,
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_server_version(),
                      value: server?.version,
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_compat(),
                      value: startCase(data?.compat.kind),
                    },
                    {
                      name: m.settings_system_proxy_system_service_ctrl_detail_restart_attempts(),
                      value: data?.restart_attempts,
                    },
                  ]}
                />

                {core && (
                  <ServiceDetailSection
                    title={m.settings_system_proxy_system_service_ctrl_detail_core()}
                    items={[
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_core_type(),
                        value: formatCoreType(core.type),
                      },
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_core_state(),
                        value: formatCoreState(core.state),
                      },
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_core_health(),
                        value: core.health?.state,
                      },
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_core_controller(),
                        value: formatCoreController(core.controller),
                      },
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_core_config_path(),
                        value: core.config_path,
                      },
                    ]}
                  />
                )}

                {server && (
                  <ServiceDetailSection
                    title={m.settings_system_proxy_system_service_ctrl_detail_paths()}
                    items={[
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_service_data_dir(),
                        value: server.runtime_infos.service_data_dir,
                      },
                      {
                        name: m.settings_system_proxy_system_service_ctrl_detail_service_config_dir(),
                        value: server.runtime_infos.service_config_dir,
                      },
                    ]}
                  />
                )}
              </div>

              {showRawJson && <ServiceDetailRawJson code={rawJson} />}
            </CardContent>
          </ScrollArea>

          <CardFooter className="shrink-0 gap-2">
            <ModalClose>{m.common_close()}</ModalClose>

            <Button variant="flat" onClick={() => setShowRawJson((v) => !v)}>
              {showRawJson
                ? m.settings_system_proxy_system_service_ctrl_detail_hide_raw_json()
                : m.settings_system_proxy_system_service_ctrl_detail_show_raw_json()}
            </Button>
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

      return `cd "${coreDir}"\n${!isWindows ? 'sudo ' : ''}./nyanpasu-service ${operation}`
    }
    return ''
  }, [systemService?.status, serviceInstallPrompt, coreDir])

  const handleCopyToClipboard = useLockFn(async () => {
    if (!userOperationCommands) {
      return
    }

    await writeClipboardText(userOperationCommands)
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

import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { useEffect, useId, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  LauncherMenu,
  LauncherMenuGroup,
  LauncherMenuIconButton,
  LauncherMenuRow,
} from '@nyanpasu/ui/launcher-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@nyanpasu/ui/popover'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { WidgetInstanceProvider } from './core/widget-context'
import { useDashboardContext } from './provider'
import { getWidgetDefinition } from './registry'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WidgetConfig,
  WidgetId,
} from './widget-config'

export function WidgetConfigSaveStatus() {
  const { saveStatus, configLoading, configReadError, retryConfig } =
    useDashboardContext()
  const failed = configReadError || saveStatus.state === 'error'

  if (!configLoading && !failed) return null

  return (
    <div
      className="text-on-surface-variant flex items-center justify-between gap-2 text-xs"
      data-slot="widget-config-save-status"
    >
      <span
        role="status"
        aria-live="polite"
        className={failed ? 'text-error' : undefined}
      >
        {configLoading
          ? m.dashboard_widget_config_loading()
          : configReadError
            ? m.dashboard_widget_config_load_failed()
            : m.dashboard_widget_config_save_failed()}
      </span>
      {failed && (
        <Button
          className="h-7 min-w-0 px-2 text-xs"
          disabled={configLoading || saveStatus.state === 'saving'}
          onClick={retryConfig}
        >
          {m.dashboard_widget_config_retry()}
        </Button>
      )}
    </div>
  )
}

/**
 * The shared config surface. Each migrated widget owns its fields through
 * `definition.ConfigForm`; this shell only supplies the popover, reset action,
 * persistence state, and the instance context the form reads.
 */
export default function WidgetConfigMenu({
  id,
  type,
  isActive = true,
}: {
  id: string
  type: WidgetId
  isActive?: boolean
}) {
  const { configs, saveConfig, saveStatus, configLoading, configReadError } =
    useDashboardContext()
  const config = getWidgetConfig(configs, id, type)
  const definition = getWidgetDefinition(type)
  const ConfigForm = definition.ConfigForm
  const disabled =
    configLoading || configReadError || saveStatus.state === 'saving'
  const titleId = useId()
  const reducedMotion = useReducedMotion()
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!isActive) {
      setOpen(false)
    }
  }, [isActive])

  return (
    <Popover open={isActive && open} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <motion.button
              type="button"
              aria-label={m.dashboard_widget_config_title()}
              data-slot="widget-config-trigger"
              className="border-outline/30 bg-surface text-on-surface hover:bg-on-surface/8 data-[state=open]:bg-primary-container data-[state=open]:text-on-primary-container focus-visible:ring-primary pointer-events-auto absolute -bottom-1 left-1/2 z-20 grid h-6 w-9 -translate-x-1/2 cursor-pointer place-items-center rounded-full border shadow-sm outline-none focus-visible:ring-2"
              onPointerDown={(event) => event.stopPropagation()}
              onKeyDown={(event) => event.stopPropagation()}
              onClick={(event) => event.stopPropagation()}
              whileHover={reducedMotion ? undefined : { scale: 1.06 }}
              whileTap={reducedMotion ? undefined : { scale: 0.94 }}
            >
              <TuneRounded className="size-3.5" aria-hidden="true" />
            </motion.button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{m.dashboard_widget_config_title()}</TooltipContent>
      </Tooltip>
      <PopoverContent
        aria-labelledby={titleId}
        className="w-80 overflow-visible border-0 bg-transparent shadow-none"
        onKeyDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <LauncherMenu
          className="min-h-0 flex-auto"
          data-slot="widget-config-menu"
        >
          <LauncherMenuGroup data-slot="widget-config-header">
            <LauncherMenuRow className="justify-between">
              <h2 id={titleId} className="text-sm font-medium">
                {m.dashboard_widget_config_title()}
              </h2>

              <LauncherMenuIconButton
                aria-label={m.dashboard_widget_config_reset()}
                title={m.dashboard_widget_config_reset()}
                disabled={disabled}
                onClick={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
              >
                <RestartAltRounded className="size-5" aria-hidden="true" />
              </LauncherMenuIconButton>
            </LauncherMenuRow>
          </LauncherMenuGroup>

          <ScrollArea
            className="min-h-0 flex-auto rounded-3xl"
            data-slot="widget-config-scroll-area"
          >
            {ConfigForm && (
              <WidgetInstanceProvider
                id={id}
                type={type}
                mode="live"
                interactive={false}
                isEditing
                span={{
                  w: definition.size.defaultW,
                  h: definition.size.defaultH,
                }}
                definition={definition}
                config={config}
                configDisabled={disabled}
                updateConfig={(patch) =>
                  saveConfig(id, { ...config, ...patch } as WidgetConfig)
                }
                resetConfig={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
              >
                <ConfigForm />
              </WidgetInstanceProvider>
            )}
          </ScrollArea>

          {(configLoading ||
            configReadError ||
            saveStatus.state === 'error') && (
            <LauncherMenuGroup data-slot="widget-config-footer">
              <LauncherMenuRow className="py-3">
                <WidgetConfigSaveStatus />
              </LauncherMenuRow>
            </LauncherMenuGroup>
          )}
        </LauncherMenu>
      </PopoverContent>
    </Popover>
  )
}

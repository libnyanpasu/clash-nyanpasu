import TuneRounded from '~icons/material-symbols/tune-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { useId } from 'react'
import { Button } from '@/components/ui/button'
import { useDndGridContext } from '@/components/ui/dnd-grid/context'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { Switch } from '@/components/ui/switch'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import { useDashboardContext } from './provider'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  PROXY_HORIZONTAL_MIN_WIDTH,
  WidgetConfig,
  WidgetId,
} from './widget-config'

function Choice<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string
  value: T
  options: { value: T; label: string; disabled?: boolean }[]
  disabled: boolean
  onChange: (value: T) => void
}) {
  return (
    <div className="space-y-2" data-slot="widget-config-choice">
      <div className="text-on-surface-variant text-xs">{label}</div>
      <SegmentedButton
        size="sm"
        value={value}
        disabled={disabled}
        aria-label={label}
        onValueChange={(next) => {
          const option = options.find((item) => item.value === next)
          if (option) onChange(option.value)
        }}
      >
        {options.map((option) => (
          <SegmentedButtonItem
            key={option.value}
            value={option.value}
            disabled={option.disabled}
            hideIndicator
          >
            {option.label}
          </SegmentedButtonItem>
        ))}
      </SegmentedButton>
    </div>
  )
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string
  checked: boolean
  disabled: boolean
  onChange: (value: boolean) => void
}) {
  const id = useId()
  return (
    <div
      className="flex items-center justify-between gap-3"
      data-slot="widget-config-toggle"
    >
      <label htmlFor={id} className="text-sm">
        {label}
      </label>
      <Switch
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
      />
    </div>
  )
}

function ConfigFields({
  config,
  disabled,
  wideEnough,
  onChange,
}: {
  config: WidgetConfig
  disabled: boolean
  wideEnough: boolean
  onChange: (config: WidgetConfig) => void
}) {
  switch (config.type) {
    case WidgetId.ProxyShortcuts:
      return (
        <>
          <Choice
            label={m.dashboard_widget_proxy_shortcuts_config_orientation()}
            value={config.orientation}
            disabled={disabled}
            options={[
              {
                value: 'vertical',
                label: m.dashboard_widget_proxy_shortcuts_config_vertical(),
              },
              {
                value: 'horizontal',
                label: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
                disabled: config.buttons === 'both' && !wideEnough,
              },
            ]}
            onChange={(orientation) => onChange({ ...config, orientation })}
          />
          {!wideEnough && config.buttons === 'both' && (
            <p className="text-on-surface-variant text-xs">
              {m.dashboard_widget_proxy_shortcuts_config_widen({
                columns: PROXY_HORIZONTAL_MIN_WIDTH,
              })}
            </p>
          )}
          <Choice
            label={m.dashboard_widget_proxy_shortcuts_config_buttons()}
            value={config.buttons}
            disabled={disabled}
            options={[
              {
                value: 'both',
                label: m.dashboard_widget_proxy_shortcuts_config_both(),
                disabled: config.orientation === 'horizontal' && !wideEnough,
              },
              {
                value: 'system',
                label: m.dashboard_widget_proxy_shortcuts_config_system(),
              },
              { value: 'tun', label: 'TUN' },
            ]}
            onChange={(buttons) => onChange({ ...config, buttons })}
          />
          {config.buttons === 'both' && (
            <Choice
              label={m.dashboard_widget_proxy_shortcuts_config_order()}
              value={config.order}
              disabled={disabled}
              options={[
                {
                  value: 'system-first',
                  label:
                    m.dashboard_widget_proxy_shortcuts_config_system_first(),
                },
                {
                  value: 'tun-first',
                  label: m.dashboard_widget_proxy_shortcuts_config_tun_first(),
                },
              ]}
              onChange={(order) => onChange({ ...config, order })}
            />
          )}
        </>
      )
    case WidgetId.TrafficDown:
    case WidgetId.TrafficUp:
      return (
        <>
          <Toggle
            label={m.dashboard_widget_sparkline_config_chart()}
            checked={config.showChart}
            disabled={disabled}
            onChange={(showChart) => onChange({ ...config, showChart })}
          />
          <Toggle
            label={m.dashboard_widget_traffic_config_total()}
            checked={config.showTotal}
            disabled={disabled}
            onChange={(showTotal) => onChange({ ...config, showTotal })}
          />
          <Choice
            label={m.dashboard_widget_traffic_config_speed_unit()}
            value={config.unit}
            disabled={disabled}
            options={[
              { value: 'bytes', label: 'B/s' },
              { value: 'bits', label: 'bit/s' },
            ]}
            onChange={(unit) => onChange({ ...config, unit })}
          />
        </>
      )
    case WidgetId.Memory:
    case WidgetId.Connections:
      return (
        <>
          <Toggle
            label={m.dashboard_widget_sparkline_config_chart()}
            checked={config.showChart}
            disabled={disabled}
            onChange={(showChart) => onChange({ ...config, showChart })}
          />
          <Choice
            label={m.dashboard_widget_sparkline_config_samples()}
            value={String(config.samples)}
            disabled={disabled || !config.showChart}
            options={[
              { value: '8', label: '8' },
              { value: '16', label: '16' },
              { value: '32', label: '32' },
            ]}
            onChange={(samples) =>
              onChange({ ...config, samples: Number(samples) as 8 | 16 | 32 })
            }
          />
        </>
      )
    case WidgetId.CoreShortcuts:
      return (
        <>
          <Choice
            label={m.dashboard_widget_core_shortcuts_config_density()}
            value={config.density}
            disabled={disabled}
            options={[
              {
                value: 'detailed',
                label: m.dashboard_widget_core_shortcuts_config_detailed(),
              },
              {
                value: 'compact',
                label: m.dashboard_widget_core_shortcuts_config_compact(),
              },
            ]}
            onChange={(density) => onChange({ ...config, density })}
          />
          <Toggle
            label={m.dashboard_widget_core_shortcuts_config_version()}
            checked={config.showVersion}
            disabled={disabled}
            onChange={(showVersion) => onChange({ ...config, showVersion })}
          />
          <Toggle
            label={m.dashboard_widget_core_shortcuts_config_channel()}
            checked={config.showChannel}
            disabled={disabled}
            onChange={(showChannel) => onChange({ ...config, showChannel })}
          />
        </>
      )
  }
}

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

export default function WidgetConfigMenu({
  id,
  type,
}: {
  id: string
  type: WidgetId
}) {
  const { configs, saveConfig, saveStatus, configLoading, configReadError } =
    useDashboardContext()
  const { displayItems } = useDndGridContext()
  const config = getWidgetConfig(configs, id, type)
  const disabled =
    configLoading || configReadError || saveStatus.state === 'saving'
  const wideEnough =
    (displayItems.find((item) => item.id === id)?.w ?? 0) >=
    PROXY_HORIZONTAL_MIN_WIDTH
  const titleId = useId()
  const reducedMotion = useReducedMotion()

  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <motion.button
              type="button"
              aria-label={m.dashboard_widget_config_title()}
              data-slot="widget-config-trigger"
              className="border-outline/30 bg-surface text-on-surface hover:bg-surface-variant data-[state=open]:bg-primary-container data-[state=open]:text-on-primary-container focus-visible:ring-primary absolute -bottom-1 left-1/2 z-20 grid h-6 w-9 -translate-x-1/2 cursor-pointer place-items-center rounded-full border shadow-sm outline-none focus-visible:ring-2"
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
        onKeyDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <div
          className="flex min-h-0 flex-auto flex-col"
          data-slot="widget-config-menu"
        >
          <h2
            id={titleId}
            className="shrink-0 px-4 pt-4 pb-4 text-sm font-semibold"
          >
            {m.dashboard_widget_config_title()}
          </h2>
          <ScrollArea
            className="flex-auto"
            data-slot="widget-config-scroll-area"
          >
            <div
              className="space-y-4 px-4 pb-4"
              data-slot="widget-config-fields"
            >
              <ConfigFields
                config={config}
                disabled={disabled}
                wideEnough={wideEnough}
                onChange={(next) => saveConfig(id, next)}
              />
            </div>
          </ScrollArea>
          <div
            className="border-outline-variant/50 mx-4 shrink-0 space-y-2 border-t pt-3 pb-4"
            data-slot="widget-config-footer"
          >
            <Button
              className="h-8 w-full text-xs"
              disabled={disabled}
              onClick={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
            >
              {m.dashboard_widget_config_reset()}
            </Button>
            <WidgetConfigSaveStatus />
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}

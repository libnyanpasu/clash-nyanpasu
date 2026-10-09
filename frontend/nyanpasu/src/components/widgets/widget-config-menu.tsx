import TuneRounded from '~icons/material-symbols/tune-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { useEffect, useId, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { NumberStepper } from '@nyanpasu/ui/number-stepper'
import { Popover, PopoverContent, PopoverTrigger } from '@nyanpasu/ui/popover'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { useDashboardContext } from './provider'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetConfig,
  WidgetId,
} from './widget-config'
import {
  ProviderReferencesField,
  ReportProfileField,
  SubscriptionTargetField,
  WidgetOptionSelect,
} from './widget-reference-fields'

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
  onChange,
}: {
  config: WidgetConfig
  disabled: boolean
  onChange: (config: WidgetConfig) => void
}) {
  switch (config.type) {
    case WidgetId.SubscriptionQuota:
    case WidgetId.SubscriptionSchedule:
      return (
        <>
          <SubscriptionTargetField
            target={config.target}
            disabled={disabled}
            onChange={(target) => onChange({ ...config, target })}
          />
          {config.type === WidgetId.SubscriptionSchedule ? (
            <Toggle
              label={m.dashboard_widget_config_recent_runs()}
              checked={config.showRecentRuns}
              disabled={disabled}
              onChange={(showRecentRuns) =>
                onChange({ ...config, showRecentRuns })
              }
            />
          ) : (
            <>
              <Toggle
                label={m.dashboard_widget_config_expiry()}
                checked={config.showExpiry}
                disabled={disabled}
                onChange={(showExpiry) => onChange({ ...config, showExpiry })}
              />
              <Toggle
                label={m.dashboard_widget_config_progress()}
                checked={config.showProgress}
                disabled={disabled}
                onChange={(showProgress) =>
                  onChange({ ...config, showProgress })
                }
              />
              <Choice
                label={m.dashboard_widget_subscription_quota_config_wave_style()}
                value={config.waveStyle}
                options={[
                  {
                    value: 'single',
                    label:
                      m.dashboard_widget_subscription_quota_config_wave_single(),
                  },
                  {
                    value: 'double',
                    label:
                      m.dashboard_widget_subscription_quota_config_wave_double(),
                  },
                ]}
                disabled={disabled || !config.showProgress}
                onChange={(waveStyle) => onChange({ ...config, waveStyle })}
              />
              <Toggle
                label={m.dashboard_widget_subscription_quota_config_wave_animation()}
                checked={config.animateWave}
                disabled={disabled || !config.showProgress}
                onChange={(animateWave) => onChange({ ...config, animateWave })}
              />
              <NumberStepper
                variant="filled"
                label={m.dashboard_widget_config_expiry_threshold()}
                value={config.expiryWarningDays}
                min={WIDGET_CONFIG_NUMBER_RANGES.expiryWarningDays[0]}
                max={WIDGET_CONFIG_NUMBER_RANGES.expiryWarningDays[1]}
                decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_config_expiry_threshold()}`}
                incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_config_expiry_threshold()}`}
                disabled={disabled}
                onChange={(expiryWarningDays) =>
                  onChange({ ...config, expiryWarningDays })
                }
              />
              <NumberStepper
                variant="filled"
                label={m.dashboard_widget_config_quota_threshold()}
                value={config.quotaWarningPercent}
                min={WIDGET_CONFIG_NUMBER_RANGES.quotaWarningPercent[0]}
                max={WIDGET_CONFIG_NUMBER_RANGES.quotaWarningPercent[1]}
                decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_config_quota_threshold()}`}
                incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_config_quota_threshold()}`}
                disabled={disabled}
                onChange={(quotaWarningPercent) =>
                  onChange({ ...config, quotaWarningPercent })
                }
              />
            </>
          )}
        </>
      )
    case WidgetId.ProxyMode:
      return (
        <Choice
          label={m.dashboard_widget_proxy_mode_config_layout()}
          value={config.layout}
          disabled={disabled}
          options={[
            {
              value: 'focus',
              label: m.dashboard_widget_proxy_mode_config_focus(),
            },
            {
              value: 'flex',
              label: m.dashboard_widget_proxy_mode_config_flex(),
            },
          ]}
          onChange={(layout) => onChange({ ...config, layout })}
        />
      )
    case WidgetId.RecentTraffic:
    case WidgetId.OriginTraffic:
    case WidgetId.ExitTraffic:
    case WidgetId.TargetTraffic:
    case WidgetId.RuleTraffic:
      return (
        <>
          <WidgetOptionSelect
            label={m.dashboard_widget_config_range()}
            value={config.range}
            disabled={disabled}
            options={[
              {
                value: 'last_hour',
                label: m.dashboard_widget_config_range_hour(),
              },
              {
                value: 'last6_hours',
                label: m.dashboard_widget_config_range_6hours(),
              },
              {
                value: 'last24_hours',
                label: m.dashboard_widget_config_range_24hours(),
              },
              {
                value: 'last7_days',
                label: m.dashboard_widget_config_range_7days(),
              },
              {
                value: 'last30_days',
                label: m.dashboard_widget_config_range_30days(),
              },
              { value: 'all', label: m.dashboard_widget_config_range_all() },
            ]}
            onChange={(range) =>
              onChange({ ...config, range: range as typeof config.range })
            }
          />
          <ReportProfileField
            profileUid={config.profileUid}
            disabled={disabled}
            onChange={(profileUid) => onChange({ ...config, profileUid })}
          />
          <Toggle
            label={m.dashboard_widget_config_directions()}
            checked={config.showDirections}
            disabled={disabled}
            onChange={(showDirections) =>
              onChange({ ...config, showDirections })
            }
          />
          {config.type !== WidgetId.RecentTraffic && (
            <>
              <NumberStepper
                variant="filled"
                label={m.dashboard_widget_config_top()}
                value={config.topN}
                min={WIDGET_CONFIG_NUMBER_RANGES.topN[0]}
                max={WIDGET_CONFIG_NUMBER_RANGES.topN[1]}
                decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_config_top()}`}
                incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_config_top()}`}
                disabled={disabled}
                onChange={(topN) => onChange({ ...config, topN })}
              />
              {(config.type === WidgetId.OriginTraffic ||
                config.type === WidgetId.TargetTraffic) && (
                <Toggle
                  label={m.dashboard_widget_config_hide_names()}
                  checked={config.hideNames}
                  disabled={disabled}
                  onChange={(hideNames) => onChange({ ...config, hideNames })}
                />
              )}
            </>
          )}
        </>
      )
    case WidgetId.ActiveConnections:
      return (
        <>
          <Choice
            label={m.dashboard_widget_config_sort()}
            value={config.sort}
            options={[
              {
                value: 'download',
                label: m.dashboard_widget_config_download(),
              },
              { value: 'upload', label: m.dashboard_widget_config_upload() },
              { value: 'total', label: m.dashboard_widget_config_total() },
            ]}
            disabled={disabled}
            onChange={(sort) => onChange({ ...config, sort })}
          />
          <NumberStepper
            variant="filled"
            label={m.dashboard_widget_config_top()}
            value={config.topN}
            min={WIDGET_CONFIG_NUMBER_RANGES.topN[0]}
            max={WIDGET_CONFIG_NUMBER_RANGES.topN[1]}
            decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_config_top()}`}
            incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_config_top()}`}
            disabled={disabled}
            onChange={(topN) => onChange({ ...config, topN })}
          />
          <Toggle
            label={m.dashboard_widget_config_process()}
            checked={config.showProcess}
            disabled={disabled}
            onChange={(showProcess) => onChange({ ...config, showProcess })}
          />
          <Toggle
            label={m.dashboard_widget_config_hide_targets()}
            checked={config.hideTargets}
            disabled={disabled}
            onChange={(hideTargets) => onChange({ ...config, hideTargets })}
          />
        </>
      )
    case WidgetId.ProviderUpdates:
      return (
        <>
          <Choice
            label={m.dashboard_widget_config_provider_types()}
            value={config.kinds}
            options={[
              {
                value: 'both',
                label: m.dashboard_widget_config_both_providers(),
              },
              {
                value: 'proxy',
                label: m.dashboard_widget_config_proxy_providers(),
              },
              {
                value: 'rule',
                label: m.dashboard_widget_config_rule_providers(),
              },
            ]}
            disabled={disabled}
            onChange={(kinds) => onChange({ ...config, kinds })}
          />
          <NumberStepper
            variant="filled"
            label={m.dashboard_widget_config_items()}
            value={config.maxItems}
            min={WIDGET_CONFIG_NUMBER_RANGES.maxItems[0]}
            max={WIDGET_CONFIG_NUMBER_RANGES.maxItems[1]}
            decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_config_items()}`}
            incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_config_items()}`}
            disabled={disabled}
            onChange={(maxItems) => onChange({ ...config, maxItems })}
          />
          <ProviderReferencesField
            resources={config.resources}
            disabled={disabled}
            onChange={(resources) => onChange({ ...config, resources })}
          />
        </>
      )
    case WidgetId.ProxyShortcuts:
      return (
        <>
          <Choice
            label={m.dashboard_widget_proxy_mode_config_layout()}
            value={config.layout}
            disabled={disabled}
            options={[
              {
                value: 'equal',
                label: m.dashboard_widget_proxy_shortcuts_config_equal(),
              },
              {
                value: 'flex',
                label: m.dashboard_widget_proxy_mode_config_flex(),
              },
            ]}
            onChange={(layout) => onChange({ ...config, layout })}
          />
          <Choice
            label={m.dashboard_widget_proxy_shortcuts_config_buttons()}
            value={config.buttons}
            disabled={disabled}
            options={[
              {
                value: 'both',
                label: m.dashboard_widget_proxy_shortcuts_config_both(),
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
          <NumberStepper
            variant="filled"
            label={m.dashboard_widget_sparkline_config_samples()}
            value={config.samples}
            min={WIDGET_CONFIG_NUMBER_RANGES.samples[0]}
            max={WIDGET_CONFIG_NUMBER_RANGES.samples[1]}
            decrementLabel={`${m.dashboard_widget_config_decrease()} ${m.dashboard_widget_sparkline_config_samples()}`}
            incrementLabel={`${m.dashboard_widget_config_increase()} ${m.dashboard_widget_sparkline_config_samples()}`}
            disabled={disabled || !config.showChart}
            onChange={(samples) => onChange({ ...config, samples })}
          />
        </>
      )
    case WidgetId.CoreShortcuts:
      return (
        <>
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
  isActive = true,
}: {
  id: string
  type: WidgetId
  isActive?: boolean
}) {
  const { configs, saveConfig, saveStatus, configLoading, configReadError } =
    useDashboardContext()
  const config = getWidgetConfig(configs, id, type)
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
              className="border-outline/30 bg-surface text-on-surface hover:bg-surface-variant data-[state=open]:bg-primary-container data-[state=open]:text-on-primary-container focus-visible:ring-primary pointer-events-auto absolute -bottom-1 left-1/2 z-20 grid h-6 w-9 -translate-x-1/2 cursor-pointer place-items-center rounded-full border shadow-sm outline-none focus-visible:ring-2"
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
          <h2 id={titleId} className="shrink-0 px-4 pt-4 text-sm font-semibold">
            {m.dashboard_widget_config_title()}
          </h2>
          <ScrollArea
            className="flex-auto"
            data-slot="widget-config-scroll-area"
          >
            <div
              className="space-y-4 px-4 py-4"
              data-slot="widget-config-fields"
            >
              <ConfigFields
                config={config}
                disabled={disabled}
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

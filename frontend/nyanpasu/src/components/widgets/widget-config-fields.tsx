import SwapHorizRounded from '~icons/material-symbols/swap-horiz-rounded'
import { useId } from 'react'
import { LauncherMenuChipButton } from '@nyanpasu/ui/launcher-menu'
import { NumberStepper } from '@nyanpasu/ui/number-stepper'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { ConfigRow } from './widget-config-row'
import { WidgetOptionSelect } from './widget-reference-fields'

/** A labelled switch row for a boolean widget option. */
export function ConfigToggle({
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

/**
 * A labelled choice. Two-way choices flip in place; longer lists use the
 * shared select so every widget form has one control pattern.
 */
export function ConfigChoice<T extends string>({
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
  const selected = options.find((option) => option.value === value)
  const other = options.find((option) => option.value !== value)

  if (options.length === 2) {
    return (
      <ConfigRow label={label}>
        <LauncherMenuChipButton
          aria-label={`${label}: ${selected?.label ?? value}`}
          disabled={disabled || !other || other.disabled}
          onClick={() => other && onChange(other.value)}
        >
          <span className="truncate">{selected?.label ?? value}</span>

          <SwapHorizRounded className="size-4 shrink-0" aria-hidden="true" />
        </LauncherMenuChipButton>
      </ConfigRow>
    )
  }

  return (
    <WidgetOptionSelect
      label={label}
      value={value}
      options={options}
      disabled={disabled}
      onChange={(next) => {
        const option = options.find((item) => item.value === next)
        if (option) onChange(option.value)
      }}
    />
  )
}

/** A bounded integer row backed by the shared number stepper. */
export function ConfigStepper({
  label,
  value,
  range,
  disabled,
  onChange,
}: {
  label: string
  value: number
  range: readonly [number, number]
  disabled: boolean
  onChange: (value: number) => void
}) {
  return (
    <NumberStepper
      variant="filled"
      layout="inline"
      label={label}
      value={value}
      min={range[0]}
      max={range[1]}
      decrementLabel={`${m.dashboard_widget_config_decrease()} ${label}`}
      incrementLabel={`${m.dashboard_widget_config_increase()} ${label}`}
      disabled={disabled}
      onChange={onChange}
    />
  )
}

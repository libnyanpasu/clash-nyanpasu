import { useId } from 'react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import {
  getRemoteSource,
  isConfigItem,
  useClashProxiesProvider,
  useClashRulesProvider,
  useProfile,
} from '@nyanpasu/query'
import type { ProviderReference, SubscriptionTarget } from './widget-config'

export function WidgetOptionSelect({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string
  value: string
  options: { value: string; label: string }[]
  disabled: boolean
  onChange: (value: string) => void
}) {
  const selected = options.find((option) => option.value === value)
  return (
    <Select
      variant="outlined"
      value={value}
      disabled={disabled}
      onValueChange={onChange}
    >
      <SelectTrigger aria-label={label}>
        <SelectValue placeholder={label}>
          {selected?.label ?? value}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function ProfileSelect({
  value,
  remoteOnly,
  includeCurrent,
  disabled,
  onChange,
}: {
  value: string | null
  remoteOnly?: boolean
  includeCurrent?: boolean
  disabled: boolean
  onChange: (value: string | null) => void
}) {
  const { query } = useProfile()
  const items = (query.data?.items ?? []).filter(
    (item) => isConfigItem(item) && (!remoteOnly || getRemoteSource(item)),
  )
  const options = [
    {
      value: '*',
      label: includeCurrent
        ? m.dashboard_widget_config_follow_current()
        : m.dashboard_widget_config_all_profiles(),
    },
    ...items.map((item) => ({ value: item.uid, label: item.name ?? item.uid })),
  ]
  if (value && !items.some((item) => item.uid === value))
    options.push({
      value,
      label: m.dashboard_widget_config_missing_reference({ name: value }),
    })
  return (
    <div className="space-y-2">
      <WidgetOptionSelect
        label={m.dashboard_widget_config_profile()}
        value={value ?? '*'}
        options={options}
        disabled={disabled || query.isPending}
        onChange={(next) => onChange(next === '*' ? null : next)}
      />
      {query.isError && (
        <p role="status" className="text-error text-xs">
          {m.dashboard_widget_config_references_failed()}
        </p>
      )}
    </div>
  )
}

export function SubscriptionTargetField({
  target,
  disabled,
  onChange,
}: {
  target: SubscriptionTarget
  disabled: boolean
  onChange: (target: SubscriptionTarget) => void
}) {
  return (
    <ProfileSelect
      remoteOnly
      includeCurrent
      value={target.kind === 'fixed' ? target.profileUid : null}
      disabled={disabled}
      onChange={(profileUid) =>
        onChange(
          profileUid ? { kind: 'fixed', profileUid } : { kind: 'current' },
        )
      }
    />
  )
}

export function ReportProfileField({
  profileUid,
  disabled,
  onChange,
}: {
  profileUid: string | null
  disabled: boolean
  onChange: (uid: string | null) => void
}) {
  return (
    <>
      <ProfileSelect
        value={profileUid}
        disabled={disabled}
        onChange={onChange}
      />
      <p className="text-on-surface-variant text-xs">
        {m.dashboard_widget_config_profile_attribution()}
      </p>
    </>
  )
}

export function ProviderReferencesField({
  resources,
  disabled,
  onChange,
}: {
  resources: ProviderReference[]
  disabled: boolean
  onChange: (refs: ProviderReference[]) => void
}) {
  const idPrefix = useId()
  const proxies = useClashProxiesProvider()
  const rules = useClashRulesProvider()
  const available: ProviderReference[] = [
    ...Object.keys(proxies.data ?? {}).map((name) => ({
      kind: 'proxy' as const,
      name,
    })),
    ...Object.keys(rules.data ?? {}).map((name) => ({
      kind: 'rule' as const,
      name,
    })),
  ]
  const refs = [
    ...resources,
    ...available.filter(
      (ref) =>
        !resources.some(
          (entry) => entry.kind === ref.kind && entry.name === ref.name,
        ),
    ),
  ]
  return (
    <div className="space-y-2" data-slot="widget-provider-references-config">
      <p className="text-on-surface-variant text-xs">
        {m.dashboard_widget_config_resources()}
      </p>
      {(proxies.isError || rules.isError) && (
        <p className="text-error text-xs">
          {m.dashboard_widget_config_references_failed()}
        </p>
      )}
      {refs.map((ref) => {
        const checked = resources.some(
          (entry) => entry.kind === ref.kind && entry.name === ref.name,
        )
        const id = `${idPrefix}-${ref.kind}-${ref.name}`
        return (
          <div
            key={JSON.stringify(ref)}
            className="flex items-center justify-between gap-2"
          >
            <label
              htmlFor={id}
              className="min-w-0 truncate text-xs"
              title={ref.name}
            >
              {ref.kind === 'proxy'
                ? m.dashboard_widget_config_proxy_providers()
                : m.dashboard_widget_config_rule_providers()}
              : {ref.name}
              {!available.some(
                (entry) => entry.kind === ref.kind && entry.name === ref.name,
              )
                ? ` (${m.dashboard_widget_config_missing()})`
                : ''}
            </label>
            <Switch
              id={id}
              disabled={disabled}
              checked={checked}
              onCheckedChange={(next) =>
                onChange(
                  next
                    ? [...resources, ref]
                    : resources.filter(
                        (entry) =>
                          entry.kind !== ref.kind || entry.name !== ref.name,
                      ),
                )
              }
            />
          </div>
        )
      })}
    </div>
  )
}

import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import {
  useClashAccess,
  useClashConfig,
  useClashSettings,
} from '@nyanpasu/query'
import type { ManageableField } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card'

type ManagedGuardField =
  'unified-delay' | 'tcp-concurrent' | 'ipv6' | 'allow-lan'

type ManagedGuardOption = 'unmanaged' | 'enabled' | 'disabled'

const FIELD_LABELS = {
  'unified-delay': m.settings_clash_settings_unified_delay_label,
  'tcp-concurrent': m.settings_clash_settings_tcp_concurrent_label,
  ipv6: m.settings_clash_settings_ipv6_label,
  'allow-lan': m.settings_clash_settings_allow_lan_label,
} satisfies Record<ManagedGuardField, () => string>

const OPTION_LABELS = {
  unmanaged: m.settings_clash_settings_managed_guard_field_unmanaged,
  enabled: m.settings_clash_settings_managed_guard_field_enabled,
  disabled: m.settings_clash_settings_managed_guard_field_disabled,
} satisfies Record<ManagedGuardOption, () => string>

const toOption = (field: ManageableField<boolean>): ManagedGuardOption => {
  if (field.kind === 'unmanaged') {
    return 'unmanaged'
  }

  return field.value ? 'enabled' : 'disabled'
}

const toField = (option: ManagedGuardOption): ManageableField<boolean> =>
  option === 'unmanaged'
    ? { kind: 'unmanaged' }
    : { kind: 'managed', value: option === 'enabled' }

/**
 * A runtime config field Nyanpasu either forces to a value or leaves to the
 * profiles, so a subscription or Merge transform can set it. `allow-lan` is
 * the controller exposure and is written through its own request.
 */
export default function ManagedGuardFieldSelector({
  field,
}: {
  field: ManagedGuardField
}) {
  const { query: clashSettings } = useClashSettings()
  const overrides = clashSettings.data?.overrides

  const { upsert: upsertSettings } = useClashConfig()
  const { upsert: upsertAccess } = useClashAccess()

  const current = useMemo(() => {
    const value = overrides?.[field]
    return value ? toOption(value) : undefined
  }, [overrides, field])

  const handleChange = useLockFn(async (option: ManagedGuardOption) => {
    try {
      if (field === 'allow-lan') {
        await upsertAccess.mutateAsync({ 'allow-lan': toField(option) })
      } else {
        await upsertSettings.mutateAsync({ [field]: toField(option) })
      }
    } catch (error) {
      message(`Update ${field} failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <SettingsCard data-slot="managed-guard-field-selector-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="managed-guard-field-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>{FIELD_LABELS[field]()}</ItemLabelText>

                  <ItemLabelDescription>
                    {current ? OPTION_LABELS[current]() : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(OPTION_LABELS).map(([option, label]) => (
            <DropdownMenuCheckboxItem
              checked={current === option}
              key={option}
              onSelect={() => handleChange(option as ManagedGuardOption)}
            >
              {label()}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}

import AddRounded from '~icons/material-symbols/add-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import EditOutlineRounded from '~icons/material-symbols/edit-outline-rounded'
import RuleRounded from '~icons/material-symbols/rule-rounded'
import { cva } from 'class-variance-authority'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { Modal, ModalTrigger } from '@nyanpasu/ui/modal'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { PlatformLinux } from '@nyanpasu/platform/react'
import { useNetworkAutomation } from '@nyanpasu/query'
import type {
  MechanismAction,
  NetworkAutomation,
  NetworkRule,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../../../../_modules/settings-card'
import { InterfaceKindIcon, interfaceKindLabel } from './interface-kind'
import { actionLabel } from './mechanism-actions-fields'
import NetworkRuleDialog from './network-rule-dialog'

const ruleActionVariants = cva(
  [
    'inline-flex h-6 items-center gap-1 rounded-lg pr-2 pl-1',
    'text-xs font-medium whitespace-nowrap',
  ],
  {
    variants: {
      action: {
        enable: 'bg-secondary-container text-on-secondary-container',
        disable: 'bg-surface-variant/60 text-on-surface-variant',
      },
    },
  },
)

const describeConditions = (rule: NetworkRule) =>
  [
    rule.conditions.wifi_ssid &&
      m.settings_system_proxy_network_automation_network_wifi({
        name: rule.conditions.wifi_ssid,
      }),
    rule.conditions.gateway_mac &&
      m.settings_system_proxy_network_automation_network_gateway({
        mac: rule.conditions.gateway_mac,
      }),
    // A Wi-Fi name already says the connection is Wi-Fi.
    rule.conditions.interface_kind &&
      !(
        rule.conditions.interface_kind === 'wifi' && rule.conditions.wifi_ssid
      ) &&
      interfaceKindLabel(rule.conditions.interface_kind),
  ]
    .filter(Boolean)
    .join(' · ')

/** What a rule does to one mechanism; nothing for a mechanism it keeps. */
function RuleAction({
  label,
  action = 'keep',
}: {
  label: string
  action?: MechanismAction
}) {
  if (action === 'keep') {
    return null
  }

  const Icon = action === 'enable' ? CheckRounded : CloseRounded

  return (
    <span
      className={ruleActionVariants({ action })}
      title={actionLabel(action)}
      data-slot="network-rule-action"
      data-action={action}
    >
      <Icon className="size-4" aria-hidden />

      {label}

      <span className="sr-only">{actionLabel(action)}</span>
    </span>
  )
}

function RuleRow({
  rule,
  onChange,
  onDelete,
}: {
  rule: NetworkRule
  onChange: (rule: NetworkRule) => Promise<void>
  onDelete: () => Promise<void>
}) {
  // A failed save is already reported; only the dialog needs to see it.
  const quietly = (save: Promise<void>) => save.catch(() => undefined)

  const enabled = rule.enabled ?? true
  const kind =
    rule.conditions.interface_kind ??
    (rule.conditions.wifi_ssid ? 'wifi' : 'any')

  return (
    <div
      className="group flex min-w-0 items-center gap-4 px-5 py-5"
      data-slot="network-rule-row"
      data-enabled={enabled}
    >
      <div
        className={cn(
          'grid size-10 shrink-0 place-content-center rounded-full',
          'bg-secondary-container text-on-secondary-container',
          'transition-opacity group-data-[enabled=false]:opacity-50',
        )}
        data-slot="network-rule-icon"
      >
        <InterfaceKindIcon kind={kind} className="size-5" />
      </div>

      <div
        className={cn(
          'flex min-w-0 flex-1 flex-col gap-0.5',
          'transition-opacity group-data-[enabled=false]:opacity-50',
        )}
        data-slot="network-rule-summary"
      >
        <span className="truncate text-base font-medium">{rule.name}</span>

        <span className="text-on-surface-variant truncate text-sm">
          {describeConditions(rule)}
        </span>

        <div className="mt-1.5 flex flex-wrap gap-1 empty:hidden">
          <RuleAction
            label={m.settings_system_proxy_system_proxy_label()}
            action={rule.actions?.system_proxy}
          />

          <RuleAction
            label={m.settings_system_proxy_tun_mode_label()}
            action={rule.actions?.tun}
          />

          <PlatformLinux>
            <RuleAction
              label={m.settings_clash_transparent_proxy_label()}
              action={rule.actions?.transparent_proxy}
            />
          </PlatformLinux>
        </div>
      </div>

      <Switch
        aria-label={rule.name}
        checked={enabled}
        onCheckedChange={(enabled) => quietly(onChange({ ...rule, enabled }))}
      />

      <div
        className="flex shrink-0 items-center"
        data-slot="network-rule-actions"
      >
        <Modal>
          <ModalTrigger asChild>
            <Button
              icon
              className="text-on-surface-variant"
              aria-label={m.settings_system_proxy_network_automation_rule_edit()}
            >
              <EditOutlineRounded className="size-5" aria-hidden />
            </Button>
          </ModalTrigger>

          <NetworkRuleDialog rule={rule} onSubmit={onChange} />
        </Modal>

        <Button
          icon
          className="text-on-surface-variant"
          aria-label={m.settings_system_proxy_network_automation_rule_delete()}
          onClick={() => quietly(onDelete())}
        >
          <DeleteOutlineRounded className="size-5" aria-hidden />
        </Button>
      </div>
    </div>
  )
}

/**
 * The ordered rules, one settings card each; the first enabled match decides.
 * The cards are animated items so they enter and leave with their group.
 */
export default function NetworkRuleList() {
  const { policy, save } = useNetworkAutomation()
  const rules = policy?.rules ?? []

  const saveRules = async (next: NetworkRule[]) => {
    if (!policy) {
      return
    }

    const nextPolicy: NetworkAutomation = { ...policy, rules: next }

    try {
      await save(nextPolicy)
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })

      throw error
    }
  }

  return (
    <>
      <SettingsCard asChild>
        <SettingsCardAnimatedItem>
          <SettingsCardContent>
            <ItemContainer data-slot="network-rule-list-header">
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_system_proxy_network_automation_rule_list_label()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {m.settings_system_proxy_network_automation_rule_list_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <Modal>
                <ModalTrigger asChild>
                  <Button
                    variant="flat"
                    className="flex shrink-0 items-center gap-1 pr-5 pl-4"
                  >
                    <AddRounded className="size-6" aria-hidden />

                    <span>
                      {m.settings_system_proxy_network_automation_rule_add()}
                    </span>
                  </Button>
                </ModalTrigger>

                <NetworkRuleDialog
                  onSubmit={(rule) => saveRules([...rules, rule])}
                />
              </Modal>
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCardAnimatedItem>
      </SettingsCard>

      {rules.length === 0 && (
        <SettingsCard asChild>
          <SettingsCardAnimatedItem>
            <EmptyState
              tone="normal"
              className="min-h-36"
              icon={RuleRounded}
              message={m.settings_system_proxy_network_automation_rule_empty()}
            />
          </SettingsCardAnimatedItem>
        </SettingsCard>
      )}

      {rules.map((rule) => (
        <SettingsCard key={rule.id} asChild>
          <SettingsCardAnimatedItem>
            <RuleRow
              rule={rule}
              onChange={(changed) =>
                saveRules(
                  rules.map((item) => (item.id === rule.id ? changed : item)),
                )
              }
              onDelete={() =>
                saveRules(rules.filter((item) => item.id !== rule.id))
              }
            />
          </SettingsCardAnimatedItem>
        </SettingsCard>
      ))}
    </>
  )
}

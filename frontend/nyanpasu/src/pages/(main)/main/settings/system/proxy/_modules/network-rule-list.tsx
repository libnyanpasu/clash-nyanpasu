import DeleteRounded from '~icons/material-symbols/delete-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Modal, ModalTrigger } from '@nyanpasu/ui/modal'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useNetworkAutomation } from '@nyanpasu/query'
import type { NetworkAutomation, NetworkRule } from '@nyanpasu/rpc/types'
import NetworkRuleDialog from './network-rule-dialog'

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
    rule.conditions.interface_kind,
  ]
    .filter(Boolean)
    .join(' · ')

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

  return (
    <div
      className="flex items-center justify-between gap-2"
      data-slot="network-rule-row"
    >
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate font-medium">{rule.name}</span>

        <span className="text-on-surface-variant truncate text-sm">
          {describeConditions(rule)}
        </span>
      </div>

      <div className="flex items-center gap-1">
        <Switch
          aria-label={rule.name}
          checked={rule.enabled ?? true}
          onCheckedChange={(enabled) => quietly(onChange({ ...rule, enabled }))}
        />

        <Modal>
          <ModalTrigger asChild>
            <Button
              variant="basic"
              aria-label={m.settings_system_proxy_network_automation_rule_edit()}
            >
              <EditRounded />
            </Button>
          </ModalTrigger>

          <NetworkRuleDialog rule={rule} onSubmit={onChange} />
        </Modal>

        <Button
          variant="basic"
          aria-label={m.settings_system_proxy_network_automation_rule_delete()}
          onClick={() => quietly(onDelete())}
        >
          <DeleteRounded />
        </Button>
      </div>
    </div>
  )
}

/** The ordered rules; the first enabled match decides. */
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
    <div className="flex flex-col gap-3" data-slot="network-rule-list">
      {rules.length === 0 && (
        <p className="text-on-surface-variant text-sm">
          {m.settings_system_proxy_network_automation_rule_empty()}
        </p>
      )}

      {rules.map((rule) => (
        <RuleRow
          key={rule.id}
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
      ))}

      <Modal>
        <ModalTrigger asChild>
          <Button variant="stroked">
            {m.settings_system_proxy_network_automation_rule_add()}
          </Button>
        </ModalTrigger>

        <NetworkRuleDialog onSubmit={(rule) => saveRules([...rules, rule])} />
      </Modal>
    </div>
  )
}

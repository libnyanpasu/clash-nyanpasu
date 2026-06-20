import { useEffect, useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Modal, ModalContent, ModalTitle } from '@/components/ui/modal'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { ConnectionRow } from '..'
import {
  buildRuleLine,
  RULE_TYPES,
  RuleType,
} from '../../proxies/group/_modules/group-builder'
import { useGroupRules } from './use-group-rules'

interface RuleCandidate {
  key: string
  label: string
  value: string
  type: RuleType
}

export default function AddRuleDialog({
  data,
  open,
  onOpenChange,
}: {
  data: ConnectionRow
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { groupsQuery, addRule } = useGroupRules()

  const candidates = useMemo<RuleCandidate[]>(() => {
    const list: RuleCandidate[] = []
    const { host, destinationIP, process } = data.metadata

    if (host) {
      list.push({
        key: 'host',
        label: 'Host',
        value: host,
        type: 'DOMAIN-SUFFIX',
      })
    }

    if (destinationIP) {
      const isV6 = destinationIP.includes(':')

      list.push({
        key: 'ip',
        label: 'Destination IP',
        value: `${destinationIP}/${isV6 ? '128' : '32'}`,
        type: isV6 ? 'IP-CIDR6' : 'IP-CIDR',
      })
    }

    if (process) {
      list.push({
        key: 'process',
        label: 'Process',
        value: process,
        type: 'PROCESS-NAME',
      })
    }

    return list
  }, [data.metadata])

  const groups = groupsQuery.data ?? []

  const [fieldKey, setFieldKey] = useState('')
  const [ruleType, setRuleType] = useState<RuleType>('DOMAIN-SUFFIX')
  const [value, setValue] = useState('')
  const [targetUid, setTargetUid] = useState('')

  // Initialise from the first candidate / group each time the dialog opens.
  useEffect(() => {
    if (!open) {
      return
    }

    const candidate = candidates[0]

    setFieldKey(candidate?.key ?? '')
    setRuleType(candidate?.type ?? 'DOMAIN-SUFFIX')
    setValue(candidate?.value ?? '')
    setTargetUid(groups[0]?.uid ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const handleFieldChange = (key: string) => {
    setFieldKey(key)

    const candidate = candidates.find((item) => item.key === key)

    if (candidate) {
      setRuleType(candidate.type)
      setValue(candidate.value)
    }
  }

  const handleSubmit = useLockFn(async () => {
    const group = groups.find((item) => item.uid === targetUid)

    if (!group || !value.trim()) {
      return
    }

    try {
      await addRule.mutateAsync({
        uid: group.uid,
        ruleLine: buildRuleLine(ruleType, value.trim(), group.groupName),
      })

      onOpenChange(false)
    } catch (error) {
      message(`${m.connections_add_rule_failed()}\n${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.connections_add_rule_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            {groups.length === 0 ? (
              <div className="text-on-surface-variant py-4 text-sm">
                {m.connections_add_rule_no_groups()}
              </div>
            ) : (
              <div className="space-y-4 pt-2">
                <Select
                  variant="outlined"
                  value={fieldKey}
                  onValueChange={handleFieldChange}
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={m.connections_add_rule_field_label()}
                    />
                  </SelectTrigger>

                  <SelectContent>
                    {candidates.map((candidate) => (
                      <SelectItem key={candidate.key} value={candidate.key}>
                        {candidate.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <Select
                  variant="outlined"
                  value={ruleType}
                  onValueChange={(next) => setRuleType(next as RuleType)}
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={m.connections_add_rule_type_label()}
                    />
                  </SelectTrigger>

                  <SelectContent>
                    {RULE_TYPES.map((type) => (
                      <SelectItem key={type} value={type}>
                        {type}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <Input
                  variant="outlined"
                  label={m.connections_add_rule_value_label()}
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                />

                <Select
                  variant="outlined"
                  value={targetUid}
                  onValueChange={setTargetUid}
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={m.connections_add_rule_target_label()}
                    />
                  </SelectTrigger>

                  <SelectContent>
                    {groups.map((group) => (
                      <SelectItem key={group.uid} value={group.uid}>
                        {group.groupName}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              onClick={handleSubmit}
              loading={addRule.isPending}
              disabled={groups.length === 0 || !value.trim() || !targetUid}
            >
              {m.common_submit()}
            </Button>

            <Button
              onClick={() => onOpenChange(false)}
              disabled={addRule.isPending}
            >
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

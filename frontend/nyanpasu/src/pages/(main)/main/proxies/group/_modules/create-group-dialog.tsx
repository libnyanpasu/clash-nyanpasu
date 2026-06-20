import AddRounded from '~icons/material-symbols/add-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import dayjs from 'dayjs'
import { AnimatePresence } from 'framer-motion'
import { useMemo, useState } from 'react'
import { AnimatedItem } from '@/components/ui/animated-item'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input, NumericInput } from '@/components/ui/input'
import { Modal, ModalContent, ModalTitle } from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
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
import {
  NormalizedProfileBuilder,
  useClashProxies,
  useProfile,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import {
  buildGroupMergeYaml,
  DEFAULT_HEALTH_CHECK_INTERVAL,
  DEFAULT_HEALTH_CHECK_URL,
  generateMergeUid,
  GroupRule,
  HEALTH_CHECK_GROUP_TYPES,
  LOAD_BALANCE_STRATEGIES,
  LoadBalanceStrategy,
  PROXY_GROUP_TYPES,
  ProxyGroupType,
  RULE_TYPES,
  RuleType,
} from './group-builder'
import { useGroupSelection } from './selection'

const TYPE_LABELS: Record<ProxyGroupType, () => string> = {
  select: () => m.proxies_group_type_select(),
  'url-test': () => m.proxies_group_type_url_test(),
  fallback: () => m.proxies_group_type_fallback(),
  'load-balance': () => m.proxies_group_type_load_balance(),
}

export default function CreateGroupDialog() {
  const { createOpen, setCreateOpen, selected, count, exit } =
    useGroupSelection()

  const { query, create, patch } = useProfile()
  const { proxies } = useClashProxies()

  const [name, setName] = useState('')
  const [type, setType] = useState<ProxyGroupType>('load-balance')
  const [strategy, setStrategy] =
    useState<LoadBalanceStrategy>('consistent-hashing')
  const [url, setUrl] = useState(DEFAULT_HEALTH_CHECK_URL)
  const [interval, setIntervalValue] = useState(DEFAULT_HEALTH_CHECK_INTERVAL)
  const [injectTargets, setInjectTargets] = useState<Set<string>>(new Set())
  const [rules, setRules] = useState<GroupRule[]>([])

  // Existing groups the new group can be injected into (so it becomes reachable).
  const groupNames = useMemo(() => {
    const names = proxies.data?.groups?.map((group) => group.name) ?? []
    const global = proxies.data?.global?.name

    return global && !names.includes(global) ? [...names, global] : names
  }, [proxies.data])

  // Scope the chain to the primary (first) active profile. `merge_profiles`
  // only keeps proxy-groups from the primary profile, so this is the only
  // attachment point that reliably surfaces the new group.
  const primaryProfile = useMemo(() => {
    const primaryUid = query.data?.current?.[0]

    if (!primaryUid) {
      return undefined
    }

    const item = query.data?.items?.find((p) => p.uid === primaryUid)

    if (item && (item.type === 'local' || item.type === 'remote')) {
      return item
    }

    return undefined
  }, [query.data])

  const showHealthCheck = HEALTH_CHECK_GROUP_TYPES.includes(type)

  const handleToggle = (value: boolean) => {
    if (create.isPending || patch.isPending) {
      return
    }

    setCreateOpen(value)

    if (value) {
      setName(
        `${TYPE_LABELS[type]()} - ${dayjs().format('YYYY-MM-DD HH:mm:ss')}`,
      )
      setInjectTargets(new Set())
      setRules([])
    }
  }

  const updateRule = (index: number, patchRule: Partial<GroupRule>) =>
    setRules((prev) =>
      prev.map((rule, i) => (i === index ? { ...rule, ...patchRule } : rule)),
    )

  const removeRule = (index: number) =>
    setRules((prev) => prev.filter((_, i) => i !== index))

  const handleSubmit = useLockFn(async () => {
    if (!primaryProfile) {
      message(m.proxies_group_create_group_no_profile(), {
        title: 'Error',
        kind: 'error',
      })
      return
    }

    if (!name.trim() || count === 0) {
      return
    }

    const uid = generateMergeUid()

    const fileData = buildGroupMergeYaml({
      name: name.trim(),
      type,
      proxies: Array.from(selected),
      strategy: type === 'load-balance' ? strategy : undefined,
      url: showHealthCheck ? url.trim() || DEFAULT_HEALTH_CHECK_URL : undefined,
      interval: showHealthCheck ? interval : undefined,
      injectInto: Array.from(injectTargets),
      rules: rules.filter((rule) => rule.value.trim()),
    })

    try {
      await create.mutateAsync({
        type: 'manual',
        data: {
          item: {
            type: 'merge',
            uid,
            name: `Group: ${name.trim()}`,
            file: null,
            desc: `Auto-generated proxy group for ${primaryProfile.name}`,
            updated: null,
          } as NormalizedProfileBuilder,
          fileData,
        },
      })

      await patch.mutateAsync({
        uid: primaryProfile.uid,
        profile: {
          ...primaryProfile,
          chain: [...(primaryProfile.chain ?? []), uid],
        } as NormalizedProfileBuilder,
      })

      await proxies.refetch()

      exit()
    } catch (error) {
      message(
        `${m.proxies_group_create_group_failed()}\n${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  const isPending = create.isPending || patch.isPending

  return (
    <Modal open={createOpen} onOpenChange={handleToggle}>
      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.proxies_group_create_group_title()}</ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[70dvh]">
              <div className="space-y-4 pt-2">
                <div className="text-on-surface-variant text-sm">
                  {m.proxies_group_create_group_selected_count({
                    count: String(count),
                  })}
                </div>

                {!primaryProfile && (
                  <div className="text-error text-sm">
                    {m.proxies_group_create_group_no_profile()}
                  </div>
                )}

                <Input
                  variant="outlined"
                  label={m.proxies_group_create_group_name_label()}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />

                <Select
                  variant="outlined"
                  value={type}
                  onValueChange={(value) => setType(value as ProxyGroupType)}
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={m.proxies_group_create_group_type_label()}
                    />
                  </SelectTrigger>

                  <SelectContent>
                    {PROXY_GROUP_TYPES.map((value) => (
                      <SelectItem key={value} value={value}>
                        {TYPE_LABELS[value]()}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <AnimatePresence initial={false}>
                  {type === 'load-balance' && (
                    <AnimatedItem>
                      <Select
                        variant="outlined"
                        value={strategy}
                        onValueChange={(value) =>
                          setStrategy(value as LoadBalanceStrategy)
                        }
                      >
                        <SelectTrigger>
                          <SelectValue
                            placeholder={m.proxies_group_create_group_strategy_label()}
                          />
                        </SelectTrigger>

                        <SelectContent>
                          {LOAD_BALANCE_STRATEGIES.map((value) => (
                            <SelectItem key={value} value={value}>
                              {value}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </AnimatedItem>
                  )}
                </AnimatePresence>

                <AnimatePresence initial={false}>
                  {showHealthCheck && (
                    <AnimatedItem className="space-y-4">
                      <Input
                        variant="outlined"
                        label={m.proxies_group_create_group_url_label()}
                        value={url}
                        onChange={(e) => setUrl(e.target.value)}
                      />

                      <NumericInput
                        variant="outlined"
                        allowNegative={false}
                        min={0}
                        label={m.proxies_group_create_group_interval_label()}
                        value={interval}
                        onChange={(value) => setIntervalValue(value ?? 0)}
                      />
                    </AnimatedItem>
                  )}
                </AnimatePresence>

                {groupNames.length > 0 && (
                  <div className="space-y-2">
                    <div className="flex items-center justify-between">
                      <span className="text-on-surface-variant text-sm">
                        {m.proxies_group_create_group_inject_label()}
                      </span>

                      <Button
                        variant="raised"
                        className="h-7 px-2 text-xs"
                        onClick={() =>
                          setInjectTargets((prev) =>
                            prev.size === groupNames.length
                              ? new Set()
                              : new Set(groupNames),
                          )
                        }
                      >
                        {injectTargets.size === groupNames.length
                          ? m.proxies_group_create_group_inject_clear()
                          : m.proxies_group_create_group_inject_all()}
                      </Button>
                    </div>

                    <div className="flex flex-wrap gap-1">
                      {groupNames.map((groupName) => {
                        const active = injectTargets.has(groupName)

                        return (
                          <Button
                            key={groupName}
                            variant="raised"
                            className={cn(
                              'h-7 max-w-full px-3 text-xs',
                              active && 'bg-primary-container',
                            )}
                            onClick={() =>
                              setInjectTargets((prev) => {
                                const next = new Set(prev)

                                if (next.has(groupName)) {
                                  next.delete(groupName)
                                } else {
                                  next.add(groupName)
                                }

                                return next
                              })
                            }
                          >
                            <span className="truncate">{groupName}</span>
                          </Button>
                        )
                      })}
                    </div>
                  </div>
                )}

                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-on-surface-variant text-sm">
                      {m.proxies_group_create_group_rules_label()}
                    </span>

                    <Button
                      variant="raised"
                      className="flex h-7 items-center gap-1 px-2 text-xs"
                      onClick={() =>
                        setRules((prev) => [
                          ...prev,
                          { type: 'DOMAIN-SUFFIX', value: '' },
                        ])
                      }
                    >
                      <AddRounded className="size-4" />
                      <span>{m.proxies_group_create_group_rule_add()}</span>
                    </Button>
                  </div>

                  {rules.map((rule, index) => (
                    <div key={index} className="flex items-center gap-1">
                      <div className="w-40 shrink-0">
                        <Select
                          variant="outlined"
                          value={rule.type}
                          onValueChange={(value) =>
                            updateRule(index, { type: value as RuleType })
                          }
                        >
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>

                          <SelectContent>
                            {RULE_TYPES.map((value) => (
                              <SelectItem key={value} value={value}>
                                {value}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>

                      <Input
                        variant="outlined"
                        className="flex-1"
                        label={m.proxies_group_create_group_rule_value_label()}
                        value={rule.value}
                        onChange={(e) =>
                          updateRule(index, { value: e.target.value })
                        }
                      />

                      <Button
                        icon
                        className="size-9 shrink-0"
                        onClick={() => removeRule(index)}
                      >
                        <DeleteOutlineRounded className="size-5" />
                      </Button>
                    </div>
                  ))}
                </div>
              </div>
            </ScrollArea>
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              onClick={handleSubmit}
              loading={isPending}
              disabled={!primaryProfile || !name.trim() || count === 0}
            >
              {m.common_submit()}
            </Button>

            <Button onClick={() => handleToggle(false)} disabled={isPending}>
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

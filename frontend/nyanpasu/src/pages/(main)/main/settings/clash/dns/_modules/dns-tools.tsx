import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Input } from '@nyanpasu/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { RequireCapability } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import type { DnsResponse } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'

const RECORD_TYPES = ['A', 'AAAA', 'CNAME', 'HTTPS', 'MX', 'TXT'] as const

/** Asks the running core's resolver, so the answer reflects its `dns`. */
function DnsQueryCard() {
  const [name, setName] = useState('')

  const [recordType, setRecordType] = useState<string>('A')

  const [response, setResponse] = useState<DnsResponse | null>(null)

  const [pending, setPending] = useState(false)

  const handleQuery = useLockFn(async () => {
    setPending(true)

    try {
      setResponse(await rpc.clashApiQueryDns(name.trim(), recordType))
    } catch (error) {
      message(`${m.settings_clash_dns_query_failed()}\n${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    } finally {
      setPending(false)
    }
  })

  const answers = response?.Answer ?? []

  return (
    <SettingsCard data-slot="dns-query-card">
      <SettingsCardContent className="flex flex-col gap-3">
        <ItemLabel>
          <ItemLabelText>{m.settings_clash_dns_query_label()}</ItemLabelText>

          <ItemLabelDescription>
            {m.settings_clash_dns_query_description()}
          </ItemLabelDescription>
        </ItemLabel>

        <div
          className="flex flex-wrap items-center gap-2"
          data-slot="dns-query-form"
        >
          <Input
            className="min-w-48 flex-1"
            value={name}
            placeholder="example.com"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && name.trim()) {
                handleQuery()
              }
            }}
          />

          <Select
            variant="chip"
            value={recordType}
            onValueChange={setRecordType}
          >
            <SelectTrigger aria-label={m.settings_clash_dns_query_type_label()}>
              <SelectValue>{recordType}</SelectValue>
            </SelectTrigger>

            <SelectContent>
              {RECORD_TYPES.map((type) => (
                <SelectItem key={type} value={type}>
                  {type}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Button
            variant="flat"
            disabled={!name.trim() || pending}
            loading={pending}
            onClick={handleQuery}
          >
            {m.settings_clash_dns_query_button()}
          </Button>
        </div>

        {response && (
          <ul
            className="flex flex-col gap-1 font-mono text-sm select-text"
            data-slot="dns-query-answers"
          >
            {answers.length === 0 && (
              <li className="text-on-surface-variant">
                {m.settings_clash_dns_query_no_answer({
                  status: response.Status,
                })}
              </li>
            )}

            {answers.map((record, index) => (
              <li key={index}>
                {record.name} {record.TTL} {record.data}
              </li>
            ))}
          </ul>
        )}
      </SettingsCardContent>
    </SettingsCard>
  )
}

/** One core cache that can be cleared. */
function FlushButton({
  label,
  description,
  flush,
}: {
  label: string
  description: string
  flush: () => ReturnType<typeof rpc.clashApiFlushDnsCache>
}) {
  const handleFlush = useLockFn(async () => {
    try {
      await flush()
      message(m.settings_clash_dns_flush_succeeded(), { kind: 'info' })
    } catch (error) {
      message(`${m.settings_clash_dns_flush_failed()}\n${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <SettingsCard data-slot="dns-flush-card">
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleFlush}
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>{label}</ItemLabelText>

              <ItemLabelDescription>{description}</ItemLabelDescription>
            </ItemLabel>
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

export default function DnsTools() {
  return (
    <div data-slot="dns-tools">
      <SettingsLabel>{m.settings_clash_dns_tools_label()}</SettingsLabel>

      <SettingsGroup>
        <DnsQueryCard />

        <RequireCapability command="clash_api_flush_dns_cache">
          <FlushButton
            label={m.settings_clash_dns_flush_dns_cache_label()}
            description={m.settings_clash_dns_flush_dns_cache_description()}
            flush={rpc.clashApiFlushDnsCache}
          />
        </RequireCapability>

        <RequireCapability command="clash_api_flush_fake_ip_cache">
          <FlushButton
            label={m.settings_clash_dns_flush_fake_ip_cache_label()}
            description={m.settings_clash_dns_flush_fake_ip_cache_description()}
            flush={rpc.clashApiFlushFakeIpCache}
          />
        </RequireCapability>
      </SettingsGroup>
    </div>
  )
}

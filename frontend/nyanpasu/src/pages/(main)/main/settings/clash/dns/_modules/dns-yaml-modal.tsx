import DataObjectRounded from '~icons/material-symbols/data-object-rounded'
import { useState } from 'react'
import { parse, stringify } from 'yaml'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import {
  useDnsDraft,
  type DnsDraft,
  type DnsValues,
  type Hosts,
} from './dns-draft'

type Mapping = Record<string, unknown>

const isMapping = (value: unknown): value is Mapping =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const shapeOf = (value: unknown) =>
  Array.isArray(value) ? 'list' : value === null ? 'null' : typeof value

/** The draft as one YAML document, with `extra` keys inside `dns`. */
const toYaml = ({ dns, hosts }: DnsDraft) => {
  const { extra, ...typed } = dns

  return stringify({ dns: { ...typed, ...extra }, hosts })
}

/**
 * Reads a YAML document back into the draft. Typed keys must keep the shape
 * they have; a key left out keeps its current value. Other `dns` keys become
 * `extra`.
 */
const fromYaml = (text: string, current: DnsDraft): DnsDraft => {
  const document: unknown = parse(text) ?? {}

  if (!isMapping(document)) {
    throw new Error(m.settings_clash_dns_yaml_not_mapping({ key: 'root' }))
  }

  const dns = document.dns ?? {}
  const hosts = document.hosts ?? {}

  if (!isMapping(dns)) {
    throw new Error(m.settings_clash_dns_yaml_not_mapping({ key: 'dns' }))
  }

  if (!isMapping(hosts)) {
    throw new Error(m.settings_clash_dns_yaml_not_mapping({ key: 'hosts' }))
  }

  const next: Mapping = { ...current.dns, extra: {} }
  const extra: Mapping = {}

  for (const [key, value] of Object.entries(dns)) {
    if (key === 'extra' || !(key in current.dns)) {
      extra[key] = value
      continue
    }

    const expected = current.dns[key as keyof DnsValues]

    // `listen` and the fake-IP ranges may be unset.
    const nullable = expected === null || value === null

    if (!nullable && shapeOf(value) !== shapeOf(expected)) {
      throw new Error(
        m.settings_clash_dns_yaml_wrong_shape({
          key,
          shape: shapeOf(expected),
        }),
      )
    }

    next[key] = value
  }

  next.extra = extra

  return { dns: next as DnsValues, hosts: hosts as Hosts }
}

function DnsYamlEditor() {
  const { draft, replaceDraft } = useDnsDraft()

  const { setOpen } = useModal()

  const [text, setText] = useState(() => toYaml(draft))

  const [error, setError] = useState<string | null>(null)

  const handleApply = () => {
    try {
      replaceDraft(fromYaml(text, draft))
      setOpen(false)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return (
    <Card className="w-[min(40rem,90vw)]" data-slot="dns-yaml-editor">
      <CardHeader>
        <ModalTitle>{m.settings_clash_dns_yaml_title()}</ModalTitle>
      </CardHeader>

      <CardContent className="flex flex-col gap-2">
        <textarea
          className={cn(
            'h-[60vh] w-full resize-none rounded-lg p-3',
            'bg-surface-container-highest text-on-surface',
            'font-mono text-sm outline-hidden',
            'focus-visible:ring-primary focus-visible:ring-2',
          )}
          aria-label={m.settings_clash_dns_yaml_title()}
          spellCheck={false}
          value={text}
          onChange={(event) => {
            setText(event.target.value)
            setError(null)
          }}
          data-slot="dns-yaml-editor-input"
        />

        {error && (
          <p className="text-error text-sm" data-slot="dns-yaml-editor-error">
            {error}
          </p>
        )}
      </CardContent>

      <CardFooter className="gap-2">
        <Button variant="flat" onClick={handleApply}>
          {m.settings_clash_dns_yaml_apply()}
        </Button>

        <ModalClose>{m.common_cancel()}</ModalClose>
      </CardFooter>
    </Card>
  )
}

/**
 * Edits the draft as YAML, including keys the form has no field for. Applying
 * replaces the draft; saving stays with the page.
 */
export default function DnsYamlModal() {
  return (
    <Modal>
      <ModalTrigger asChild>
        <Button className="flex items-center gap-1 px-4" variant="stroked">
          <DataObjectRounded className="size-5" />
          <span>{m.settings_clash_dns_yaml_open()}</span>
        </Button>
      </ModalTrigger>

      <ModalContent>
        <DnsYamlEditor />
      </ModalContent>
    </Modal>
  )
}

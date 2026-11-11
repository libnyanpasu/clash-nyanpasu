import AddRounded from '~icons/material-symbols/add-rounded'
import { useRef, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input, NumericInput } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useClashSetting, useStunServerTest } from '@nyanpasu/query'
import type {
  StunAttempt,
  StunServer,
  StunServerCapabilities,
  StunServerTest,
} from '@nyanpasu/rpc/types'
import StunCapabilities from './stun-capabilities'

const DEFAULT_STUN_PORT = 3478

function attemptLabel(attempt: StunAttempt) {
  switch (attempt.kind) {
    case 'answered':
      return m.settings_public_ip_stun_result_answered({
        address: attempt.address,
        latency: attempt.latency_ms,
      })
    case 'no_address':
      return m.settings_public_ip_stun_result_no_address()
    case 'no_answer':
      return m.settings_public_ip_stun_result_no_answer()
  }
}

function TestResults({ test }: { test: StunServerTest }) {
  const rows = [
    ['UDP IPv4', test.udp_ipv4],
    ['UDP IPv6', test.udp_ipv6],
    ['TCP IPv4', test.tcp_ipv4],
    ['TCP IPv6', test.tcp_ipv6],
  ] as const

  return (
    <div className="flex flex-col gap-2" data-slot="stun-test-results">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        {rows.map(([label, attempt]) => (
          <div key={label} className="contents" data-slot="stun-test-row">
            <dt className="text-on-surface-variant">{label}</dt>
            <dd
              className={
                attempt.kind === 'answered'
                  ? 'break-all'
                  : 'text-on-surface-variant'
              }
            >
              {attemptLabel(attempt)}
            </dd>
          </div>
        ))}
      </dl>

      <div className="flex flex-wrap gap-1">
        <StunCapabilities capabilities={test.capabilities} />
      </div>
    </div>
  )
}

function answeredAny({ udp, tcp }: StunServerCapabilities) {
  return udp.ipv4 || udp.ipv6 || tcp.ipv4 || tcp.ipv6
}

/**
 * Adding a server tests it over every transport and family first; it is added
 * with what answered, and not at all when nothing did.
 */
export default function AddStunServerDialog({
  servers,
  onAdd,
}: {
  servers: StunServer[]
  /** Resolves whether the server was saved. */
  onAdd: (server: StunServer) => Promise<boolean>
}) {
  const [host, setHost] = useState('')
  const [port, setPort] = useState<number | null>(DEFAULT_STUN_PORT)
  const [added, setAdded] = useState(false)

  const { value: tunEnabled } = useClashSetting('enable_tun_mode')

  const serverTest = useStunServerTest()

  // Bumped whenever the dialog closes or its input changes, so a test that
  // answers afterwards neither saves the server nor updates the dialog.
  const submission = useRef(0)

  const discardTest = () => {
    submission.current += 1
    serverTest.reset()
  }

  const trimmedHost = host.trim()
  const portValid = port != null && port >= 1 && port <= 65535
  const duplicate = servers.some(
    (server) =>
      server.port === port &&
      server.host.toLowerCase() === trimmedHost.toLowerCase(),
  )
  const formError = !trimmedHost
    ? null
    : /\s/.test(trimmedHost)
      ? m.settings_public_ip_stun_invalid_host()
      : !portValid
        ? m.settings_public_ip_stun_invalid_port()
        : duplicate
          ? m.settings_public_ip_stun_duplicate()
          : null
  const canAdd = Boolean(trimmedHost) && portValid && formError == null
  const test = serverTest.data

  const handleOpenChange = (next: boolean) => {
    if (next) {
      setHost('')
      setPort(DEFAULT_STUN_PORT)
      setAdded(false)
      discardTest()
    } else {
      submission.current += 1
    }
  }

  const handleAdd = async () => {
    if (!canAdd || port == null) return
    const current = ++submission.current

    // A failed test is shown from the mutation's own error state.
    const result = await serverTest
      .mutateAsync({ host: trimmedHost, port })
      .catch(() => null)
    if (current !== submission.current) return
    if (!result || !answeredAny(result.capabilities)) return

    const saved = await onAdd({
      host: trimmedHost,
      port,
      enabled: true,
      capabilities: result.capabilities,
      builtin: false,
    })
    if (current !== submission.current) return
    setAdded(saved)
  }

  return (
    <Modal onOpenChange={handleOpenChange}>
      <ModalTrigger asChild>
        <Button variant="raised" className="flex items-center gap-1 px-4">
          <AddRounded className="size-6" aria-hidden />

          <span>{m.settings_public_ip_stun_add()}</span>
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card
          className="w-[28rem] max-w-full"
          data-slot="add-stun-server-dialog"
        >
          <CardHeader>
            <ModalTitle>{m.settings_public_ip_stun_add_title()}</ModalTitle>
          </CardHeader>

          <CardContent className="flex flex-col gap-4">
            <ModalDescription className="text-on-surface-variant text-sm">
              {m.settings_public_ip_stun_add_description()}
            </ModalDescription>

            <form
              className="flex gap-2"
              data-slot="add-stun-server-form"
              onSubmit={(event) => {
                event.preventDefault()
                handleAdd()
              }}
            >
              <Input
                className="flex-1"
                label={m.settings_public_ip_stun_host()}
                aria-label={m.settings_public_ip_stun_host()}
                value={host}
                disabled={added}
                autoFocus
                spellCheck={false}
                onChange={(event) => {
                  setHost(event.target.value)
                  discardTest()
                }}
              />

              <NumericInput
                className="w-28"
                label={m.settings_public_ip_stun_port()}
                aria-label={m.settings_public_ip_stun_port()}
                value={port}
                min={1}
                max={65535}
                decimalScale={0}
                allowNegative={false}
                disabled={added}
                onChange={(value) => {
                  setPort(value)
                  discardTest()
                }}
              />
            </form>

            {formError && (
              <p className="text-error text-sm" role="alert">
                {formError}
              </p>
            )}

            {tunEnabled && (
              <p className="text-on-surface-variant text-sm">
                {m.settings_public_ip_stun_tun_warning()}
              </p>
            )}

            {serverTest.error && (
              <p className="text-error text-sm" role="alert">
                {formatError(serverTest.error)}
              </p>
            )}

            {test && <TestResults test={test} />}

            {test && !answeredAny(test.capabilities) && (
              <p className="text-error text-sm" role="alert">
                {m.settings_public_ip_stun_result_none()}
              </p>
            )}
          </CardContent>

          <CardFooter className="gap-1">
            {added ? (
              <ModalClose variant="flat">
                {m.settings_public_ip_stun_added_done()}
              </ModalClose>
            ) : (
              <>
                <ModalClose>{m.common_cancel()}</ModalClose>

                <Button
                  variant="flat"
                  disabled={!canAdd}
                  loading={serverTest.isPending}
                  onClick={handleAdd}
                >
                  {m.settings_public_ip_stun_add_confirm()}
                </Button>
              </>
            )}
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

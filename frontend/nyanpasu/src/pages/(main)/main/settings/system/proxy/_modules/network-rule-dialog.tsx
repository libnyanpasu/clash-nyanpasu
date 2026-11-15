import { AnimatePresence } from 'motion/react'
import { useEffect, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import {
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  useModal,
} from '@nyanpasu/ui/modal'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { useNetworkAutomationStatus } from '@nyanpasu/query'
import type {
  InterfaceKind,
  MechanismActions,
  NetworkObservation,
  NetworkRule,
  WifiName,
} from '@nyanpasu/rpc/types'
import AnimatedErrorItem from '../../../../_modules/error-item'
import MechanismActionsFields from './mechanism-actions-fields'
import WifiNameNotice from './wifi-name-notice'

const MAC_ADDRESS = /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/i

/** Whether asking for the Wi-Fi name could still produce one. */
const needsWifiNameRead = (network: NetworkObservation) =>
  network.interface_kind === 'wifi' &&
  (network.wifi.kind === 'not_read' ||
    (network.wifi.kind === 'unavailable' &&
      network.wifi.reason === 'permission_required'))

const INTERFACE_KINDS = ['any', 'wifi', 'ethernet', 'other'] as const

const formSchema = z
  .object({
    name: z
      .string()
      .trim()
      .min(1, {
        error: () => m.settings_system_proxy_network_automation_name_required(),
      }),
    wifiSsid: z.string(),
    gatewayMac: z
      .string()
      .trim()
      .refine((value) => value === '' || MAC_ADDRESS.test(value), {
        error: () => m.settings_system_proxy_network_automation_mac_invalid(),
      }),
    interfaceKind: z.enum(INTERFACE_KINDS),
    actions: z.custom<MechanismActions>(),
  })
  .refine(
    (value) =>
      value.wifiSsid !== '' ||
      value.gatewayMac !== '' ||
      value.interfaceKind !== 'any',
    {
      path: ['wifiSsid'],
      error: () =>
        m.settings_system_proxy_network_automation_conditions_required(),
    },
  )

type FormValues = z.infer<typeof formSchema>

const emptyValues = (): FormValues => ({
  name: '',
  wifiSsid: '',
  gatewayMac: '',
  interfaceKind: 'any',
  actions: {},
})

const toFormValues = (rule: NetworkRule): FormValues => ({
  name: rule.name,
  wifiSsid: rule.conditions.wifi_ssid ?? '',
  gatewayMac: rule.conditions.gateway_mac ?? '',
  interfaceKind: rule.conditions.interface_kind ?? 'any',
  actions: rule.actions ?? {},
})

const interfaceKindLabel = (kind: (typeof INTERFACE_KINDS)[number]) => {
  switch (kind) {
    case 'any':
      return m.settings_system_proxy_network_automation_interface_any()
    case 'wifi':
      return m.settings_system_proxy_network_automation_network_wifi_kind()
    case 'ethernet':
      return m.settings_system_proxy_network_automation_network_ethernet()
    case 'other':
      return m.settings_system_proxy_network_automation_network_other()
  }
}

/**
 * Creates a rule, or edits `rule`. `onSubmit` receives the complete rule and
 * a failure leaves the dialog open.
 */
export default function NetworkRuleDialog({
  rule,
  onSubmit,
}: {
  rule?: NetworkRule
  onSubmit: (rule: NetworkRule) => Promise<void>
}) {
  const { open, setOpen } = useModal()
  const { query, requestWifiName } = useNetworkAutomationStatus()
  // The Wi-Fi name as last read for this dialog, kept to explain a missing one.
  const [wifiReading, setWifiReading] = useState<WifiName>()

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: rule ? toFormValues(rule) : emptyValues(),
  })

  useEffect(() => {
    if (open) {
      form.reset(rule ? toFormValues(rule) : emptyValues())
    }
  }, [open, rule, form])

  const current =
    query.data?.network?.state === 'online'
      ? query.data.network.network
      : undefined

  const fillFromNetwork = (network: NetworkObservation) => {
    setWifiReading(network.wifi)

    form.setValue(
      'wifiSsid',
      network.wifi.kind === 'known' ? network.wifi.ssid : '',
      { shouldDirty: true },
    )
    form.setValue('gatewayMac', network.gateway_mac ?? '', {
      shouldDirty: true,
    })
    form.setValue('interfaceKind', network.interface_kind, {
      shouldDirty: true,
    })
  }

  const handleUseCurrent = useLockFn(async () => {
    if (!current) {
      return
    }

    // The status leaves the Wi-Fi name unread until a rule matches on it, and
    // reading it is what asks the system for location access, so a rule that
    // is being created is the moment to ask.
    if (needsWifiNameRead(current)) {
      try {
        const status = await requestWifiName.mutateAsync()

        if (status.network?.state === 'online') {
          fillFromNetwork(status.network.network)
          return
        }
      } catch (error) {
        message(formatError(error), { title: 'Error', kind: 'error', error })
      }
    }

    fillFromNetwork(current)
  })

  const handleSubmit = useLockFn(
    form.handleSubmit(async (values) => {
      await onSubmit({
        id: rule?.id ?? crypto.randomUUID(),
        name: values.name,
        enabled: rule?.enabled ?? true,
        conditions: {
          wifi_ssid: values.wifiSsid || null,
          gateway_mac: values.gatewayMac.toLowerCase() || null,
          interface_kind:
            values.interfaceKind === 'any'
              ? null
              : (values.interfaceKind satisfies InterfaceKind),
        },
        actions: values.actions,
      })

      setOpen(false)
    }),
  )

  return (
    <ModalContent dismissible={!form.formState.isSubmitting}>
      <Card className="w-[28rem] max-w-full" data-slot="network-rule-dialog">
        <CardHeader>
          <ModalTitle>
            {rule
              ? m.settings_system_proxy_network_automation_rule_edit()
              : m.settings_system_proxy_network_automation_rule_add()}
          </ModalTitle>

          <ModalDescription className="text-on-surface-variant text-sm">
            {m.settings_system_proxy_network_automation_spoof_warning()}
          </ModalDescription>
        </CardHeader>

        <CardContent>
          <form
            id="network-rule-dialog-form"
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault()
              handleSubmit()
            }}
          >
            <Controller
              control={form.control}
              name="name"
              render={({ field }) => (
                <Input
                  label={m.settings_system_proxy_network_automation_name_label()}
                  variant="outlined"
                  autoFocus
                  {...field}
                />
              )}
            />

            <Controller
              control={form.control}
              name="wifiSsid"
              render={({ field }) => (
                <Input
                  label={m.settings_system_proxy_network_automation_ssid_label()}
                  variant="outlined"
                  {...field}
                />
              )}
            />

            <Controller
              control={form.control}
              name="gatewayMac"
              render={({ field }) => (
                <Input
                  label={m.settings_system_proxy_network_automation_gateway_label()}
                  variant="outlined"
                  placeholder="aa:bb:cc:dd:ee:ff"
                  {...field}
                />
              )}
            />

            <Controller
              control={form.control}
              name="interfaceKind"
              render={({ field }) => (
                <div className="flex items-center justify-between gap-4">
                  <span className="text-sm">
                    {m.settings_system_proxy_network_automation_interface_label()}
                  </span>

                  <Select
                    variant="chip"
                    value={field.value}
                    onValueChange={(kind) => field.onChange(kind)}
                  >
                    <SelectTrigger
                      aria-label={m.settings_system_proxy_network_automation_interface_label()}
                    >
                      <SelectValue>
                        {interfaceKindLabel(field.value)}
                      </SelectValue>
                    </SelectTrigger>

                    <SelectContent>
                      {INTERFACE_KINDS.map((kind) => (
                        <SelectItem key={kind} value={kind}>
                          {interfaceKindLabel(kind)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              )}
            />

            <AnimatePresence>
              {(form.formState.errors.name ||
                form.formState.errors.wifiSsid ||
                form.formState.errors.gatewayMac) && (
                <AnimatedErrorItem className="text-error text-sm">
                  {form.formState.errors.name?.message ??
                    form.formState.errors.gatewayMac?.message ??
                    form.formState.errors.wifiSsid?.message}
                </AnimatedErrorItem>
              )}
            </AnimatePresence>

            {wifiReading && (
              <WifiNameNotice
                wifi={wifiReading}
                onRequested={(status) =>
                  status.network?.state === 'online' &&
                  fillFromNetwork(status.network.network)
                }
              />
            )}

            <Button
              type="button"
              onClick={handleUseCurrent}
              disabled={!current}
              loading={requestWifiName.isPending}
            >
              {m.settings_system_proxy_network_automation_use_current()}
            </Button>

            <Controller
              control={form.control}
              name="actions"
              render={({ field }) => (
                <MechanismActionsFields
                  value={field.value}
                  onChange={(actions) => field.onChange(actions)}
                />
              )}
            />
          </form>
        </CardContent>

        <CardFooter className="gap-1">
          <Button
            type="submit"
            form="network-rule-dialog-form"
            loading={form.formState.isSubmitting}
            disabled={form.formState.isSubmitting}
          >
            {m.common_save()}
          </Button>

          <ModalClose disabled={form.formState.isSubmitting}>
            {m.common_cancel()}
          </ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
  )
}

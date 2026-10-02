import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'motion/react'
import { PropsWithChildren, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { NumericInput } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useClashConfig, useClashSetting } from '@nyanpasu/query'
import {
  type ClashApiConfig,
  type PortStrategy,
  type PortStrategyKind,
} from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const PORT_STRATEGY_KINDS = [
  'fixed',
  'allow_fallback',
  'random',
] as const satisfies PortStrategyKind[]

const STRATEGY_LABELS = {
  fixed: m.settings_clash_settings_fixed_label,
  allow_fallback: m.settings_clash_settings_allow_fallback_label,
  random: m.settings_clash_settings_random_label,
} satisfies Record<PortStrategyKind, () => string>

const STRATEGY_DESCRIPTIONS = {
  fixed: m.settings_clash_settings_fixed_description,
  allow_fallback: m.settings_clash_settings_allow_fallback_description,
  random: m.settings_clash_settings_random_description,
} satisfies Record<PortStrategyKind, () => string>

const OPTIONAL_PORTS = {
  socks_port: {
    label: m.settings_clash_settings_socks_port_label,
    runtimeKey: 'socks-port',
    defaultPort: 7891,
  },
  http_port: {
    label: m.settings_clash_settings_http_port_label,
    runtimeKey: 'port',
    defaultPort: 7892,
  },
} satisfies Record<
  string,
  { label: () => string; runtimeKey: keyof ClashApiConfig; defaultPort: number }
>

const formSchema = z.object({
  kind: z.enum(PORT_STRATEGY_KINDS),
  start_port: z.number().int().min(1).max(65535),
})

const reportError = (error: unknown) =>
  message(formatError(error), { title: 'Error', kind: 'error', error })

/** The strategy, its start port, and the port the running core bound. */
const describeStrategy = (
  strategy: PortStrategy,
  runtimePort: number | null | undefined,
) => {
  const parts: string[] = [STRATEGY_LABELS[strategy.kind]()]

  if (strategy.kind !== 'random') {
    parts.push(String(strategy.start_port))
  }

  if (
    runtimePort &&
    (strategy.kind === 'random' || runtimePort !== strategy.start_port)
  ) {
    parts.push(m.settings_clash_settings_port_current({ port: runtimePort }))
  }

  return parts.join(' · ')
}

const PortStrategyDialog = ({
  title,
  value,
  onApply,
  children,
}: PropsWithChildren<{
  title: string
  value: PortStrategy
  onApply: (strategy: PortStrategy) => Promise<void>
}>) => {
  const [open, setOpen] = useState(false)

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: value,
  })

  const kind = form.watch('kind')

  const handleOpenChange = (next: boolean) => {
    if (next) {
      form.reset(value)
    }

    setOpen(next)
  }

  const handleSubmit = form.handleSubmit(async (data) => {
    try {
      await onApply(data)

      setOpen(false)
    } catch (error) {
      reportError(error)
    }
  })

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      {children}

      <ModalContent>
        <Card className="flex min-w-96 flex-col">
          <CardHeader>
            <ModalTitle>{title}</ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
              <Controller
                name="kind"
                control={form.control}
                render={({ field }) => (
                  <div className="flex flex-col gap-2">
                    <SegmentedButton
                      value={field.value}
                      onValueChange={(next) => {
                        if (next) field.onChange(next)
                      }}
                    >
                      {PORT_STRATEGY_KINDS.map((item) => (
                        <SegmentedButtonItem key={item} value={item}>
                          {STRATEGY_LABELS[item]()}
                        </SegmentedButtonItem>
                      ))}
                    </SegmentedButton>

                    <p className="text-on-surface-variant px-4 text-sm">
                      {STRATEGY_DESCRIPTIONS[field.value]()}
                    </p>
                  </div>
                )}
              />

              <Controller
                name="start_port"
                control={form.control}
                render={({ field, fieldState }) => {
                  const handleChange = (value: number | null) => {
                    field.onChange(value)
                  }

                  return (
                    <>
                      <NumericInput
                        variant="outlined"
                        label={m.settings_clash_settings_port_number_label()}
                        value={field.value}
                        onChange={handleChange}
                        disabled={kind === 'random'}
                        allowNegative={false}
                        decimalScale={0}
                      />

                      <AnimatePresence>
                        {fieldState.error && (
                          <SettingsCardAnimatedItem className="text-error">
                            {fieldState.error.message}
                          </SettingsCardAnimatedItem>
                        )}
                      </AnimatePresence>
                    </>
                  )
                }}
              />
            </form>
          </CardContent>

          <CardFooter className="gap-2">
            <Button
              variant="flat"
              onClick={handleSubmit}
              loading={form.formState.isSubmitting}
            >
              {m.common_apply()}
            </Button>

            <ModalClose>{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

const ROW_BUTTON_CLASS =
  'text-on-surface! h-auto w-full rounded-none px-5 text-left text-base'

export function MixedPortConfig() {
  const mixedPort = useClashSetting('mixed_port')

  const clashConfig = useClashConfig()

  const strategy = mixedPort.value

  const label = (
    <ItemLabel>
      <ItemLabelText>
        {m.settings_clash_settings_mixed_port_label()}
      </ItemLabelText>

      {strategy && (
        <ItemLabelDescription>
          {describeStrategy(strategy, clashConfig.query.data?.['mixed-port'])}
        </ItemLabelDescription>
      )}
    </ItemLabel>
  )

  return (
    <SettingsCard data-slot="mixed-port-config-card">
      {strategy ? (
        <PortStrategyDialog
          title={m.settings_clash_settings_mixed_port_label()}
          value={strategy}
          onApply={(next) => mixedPort.upsert(next)}
        >
          <SettingsCardContent asChild>
            <ModalTrigger asChild>
              <Button className={ROW_BUTTON_CLASS}>
                <ItemContainer>
                  {label}

                  <ArrowForwardIosRounded />
                </ItemContainer>
              </Button>
            </ModalTrigger>
          </SettingsCardContent>
        </PortStrategyDialog>
      ) : (
        <SettingsCardContent>
          <ItemContainer>{label}</ItemContainer>
        </SettingsCardContent>
      )}
    </SettingsCard>
  )
}

/**
 * A port the core binds only when it has a strategy. The switch adds or
 * clears the strategy; the row edits it while it is set.
 */
export function OptionalPortConfig({
  field,
}: {
  field: keyof typeof OPTIONAL_PORTS
}) {
  const setting = useClashSetting(field)

  const clashConfig = useClashConfig()

  const { label, runtimeKey, defaultPort } = OPTIONAL_PORTS[field]

  const strategy = setting.value ?? null

  const handleToggle = (enabled: boolean) => {
    setting
      .upsert(enabled ? { kind: 'fixed', start_port: defaultPort } : null)
      .catch(reportError)
  }

  const itemLabel = (
    <ItemLabel>
      <ItemLabelText>{label()}</ItemLabelText>

      <ItemLabelDescription>
        {strategy
          ? describeStrategy(strategy, clashConfig.query.data?.[runtimeKey])
          : m.settings_clash_settings_port_disabled()}
      </ItemLabelDescription>
    </ItemLabel>
  )

  return (
    <SettingsCard data-slot={`${field}-config-card`}>
      <div className="flex items-stretch">
        {strategy ? (
          <PortStrategyDialog
            title={label()}
            value={strategy}
            onApply={(next) => setting.upsert(next)}
          >
            <SettingsCardContent className="min-w-0 flex-1" asChild>
              <ModalTrigger asChild>
                <Button className={ROW_BUTTON_CLASS}>{itemLabel}</Button>
              </ModalTrigger>
            </SettingsCardContent>
          </PortStrategyDialog>
        ) : (
          <SettingsCardContent className="min-w-0 flex-1">
            {itemLabel}
          </SettingsCardContent>
        )}

        <div className="flex items-center gap-4 pr-5">
          {strategy && <div className="bg-outline-variant h-8 w-px" />}

          <Switch
            checked={Boolean(strategy)}
            onCheckedChange={handleToggle}
            loading={setting.isPending}
          />
        </div>
      </div>
    </SettingsCard>
  )
}

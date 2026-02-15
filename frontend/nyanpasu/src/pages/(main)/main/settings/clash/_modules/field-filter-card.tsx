import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { PropsWithChildren, useMemo } from 'react'
import CLASH_FIELD from '@/assets/json/clash-field.json'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

type Item = {
  url?: string
  enabled: boolean
}

const OpenLinkButton = ({ data }: { data: Item }) => {
  const handleOpen = useLockFn(async () => {
    if (!data.url) {
      return
    }

    await commands.openThat(data.url)
  })

  return (
    <Button
      variant="stroked"
      className="size-6"
      icon
      onClick={(e) => {
        e.stopPropagation()
        e.preventDefault()
        handleOpen()
      }}
      asChild
    >
      <span>
        <OpenInNewRounded className="size-3" />
      </span>
    </Button>
  )
}

const FieldButton = ({
  data,
  disabled,
  label,
}: {
  data: Item
  disabled: boolean
  label: string
}) => {
  const { query, upsert } = useProfile()

  const blockTask = useBlockTask(`update-clash-field-${label}`, async () => {
    let valid = query.data?.valid ?? []

    if (data.enabled) {
      valid = valid.filter((item) => item !== label)
    } else {
      valid.push(label)
    }

    await upsert.mutateAsync({ valid })
  })

  return (
    <Button
      data-enabled={String(data.enabled)}
      className={cn(
        'flex h-12 items-center justify-between gap-2 rounded-2xl pr-3',
        'data-[enabled=true]:bg-primary-container',
        'data-[enabled=true]:dark:bg-surface-variant',
        'data-[enabled=false]:bg-primary-container/50',
        'data-[enabled=false]:dark:bg-surface-variant/10',
      )}
      disabled={disabled}
      onClick={() => blockTask.execute()}
      loading={blockTask.isPending}
    >
      <TextMarquee className="w-full min-w-0 text-left text-sm">
        {label}
      </TextMarquee>

      <div>
        <OpenLinkButton data={data} />
      </div>
    </Button>
  )
}

const ItemButton = ({
  items,
  children,
}: PropsWithChildren<{
  items: Record<string, Item>
}>) => {
  // Nyanpasu Control Fields object key
  const isNyanpasuControlField = ['default', 'handle'].includes(
    children as string,
  )

  const enableFields = Object.keys(items).filter((key) => items[key].enabled)

  return (
    <Modal>
      <ModalTrigger asChild>
        <Button
          className={cn(
            'relative h-20 w-full min-w-0 rounded-3xl',
            'bg-primary-container dark:bg-surface-variant/30',
            'flex flex-col items-start justify-center gap-0.5 pr-8',
          )}
        >
          <div className="text-base font-bold capitalize">{children}</div>

          <TextMarquee className="w-full min-w-0 text-left text-sm">
            <p className="space-x-1">
              <span>Enabled:</span>

              {enableFields.map((field) => {
                return <span key={field}>{field}</span>
              })}
            </p>
          </TextMarquee>

          <ArrowForwardIosRounded className="absolute top-1/2 right-2 size-5 -translate-y-1/2" />
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle className="capitalize">{children}</ModalTitle>

            {isNyanpasuControlField && (
              <div className="text-on-surface-variant text-sm">
                {m.settings_clash_settings_field_filter_nyanpasu_control_fields()}
              </div>
            )}
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[calc(100vh-200px)]">
              <div className="grid grid-cols-2 gap-2">
                {Object.entries(items).map(([item, data]) => {
                  return (
                    <FieldButton
                      key={item}
                      data={data}
                      disabled={isNyanpasuControlField}
                      label={item}
                    />
                  )
                })}
              </div>
            </ScrollArea>
          </CardContent>

          <CardFooter>
            <ModalClose>{m.common_close()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

export default function FieldFilterCard() {
  const { query } = useProfile()

  const mergeFields = useMemo(
    () => [
      ...Object.keys(CLASH_FIELD.default),
      ...Object.keys(CLASH_FIELD.handle),
      ...(query.data?.valid ?? []),
    ],
    [query.data],
  )

  const filteredField = (fields: Record<string, string>) => {
    const usedObjects: Record<string, Item> = {}

    for (const key in fields) {
      if (Object.prototype.hasOwnProperty.call(fields, key)) {
        usedObjects[key] = {
          url: fields[key],
          enabled: mergeFields.includes(key),
        }
      }
    }

    return usedObjects
  }

  return (
    <SettingsCard data-slot="field-filter-card">
      <SettingsCardContent
        className="flex items-center justify-between px-2"
        data-slot="field-filter-card-content"
      >
        <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
          {Object.entries(CLASH_FIELD).map(([key, value], index) => {
            const filtered = filteredField(value)

            return (
              <ItemButton key={index} items={filtered}>
                {key}
              </ItemButton>
            )
          })}
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}

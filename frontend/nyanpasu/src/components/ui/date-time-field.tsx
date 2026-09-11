import CalendarMonthRounded from '~icons/material-symbols/calendar-month-rounded'
import ChevronLeftRounded from '~icons/material-symbols/chevron-left-rounded'
import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { useEffect, useRef, useState } from 'react'
import { Button } from 'react-aria-components/Button'
import {
  Calendar,
  CalendarCell,
  CalendarGrid,
  CalendarGridBody,
  CalendarGridHeader,
  CalendarHeaderCell,
} from 'react-aria-components/Calendar'
import { DateInput, DateSegment } from 'react-aria-components/DateField'
import { DatePicker } from 'react-aria-components/DatePicker'
import { Dialog } from 'react-aria-components/Dialog'
import { FieldError } from 'react-aria-components/FieldError'
import { Group } from 'react-aria-components/Group'
import { Heading } from 'react-aria-components/Heading'
import { I18nProvider } from 'react-aria-components/I18nProvider'
import { Label } from 'react-aria-components/Label'
import { Popover } from 'react-aria-components/Popover'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { parseDateTime, toCalendarDateTime } from '@internationalized/date'
import { cn } from '@nyanpasu/utils'
import { buttonVariants } from './button'

const iconButtonClass = cn(
  buttonVariants({ icon: true }),
  'shrink-0 focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-40',
)

export default function DateTimeField({
  label,
  value,
  onChange,
}: {
  label: string
  value: string
  onChange: (value: string) => void
}) {
  const [container, setContainer] = useState<HTMLDivElement | null>(null)
  const [open, setOpen] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const wasOpen = useRef(false)
  useEffect(() => {
    const closing = wasOpen.current && !open
    wasOpen.current = open
    if (!closing) return
    // Restore after the enclosing dialog has handled the removed focus scope.
    const frame = requestAnimationFrame(() => trigger.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [open])
  return (
    <I18nProvider locale={getLocale()}>
      <div ref={setContainer}>
        <DatePicker
          isOpen={open}
          onOpenChange={setOpen}
          value={value ? parseDateTime(value) : null}
          onChange={(date) => onChange(date?.toString() ?? '')}
          granularity="minute"
          hourCycle={24}
          validationBehavior="native"
          className="group flex min-w-0 flex-col gap-1"
        >
          <Label className="text-on-surface-variant text-xs">{label}</Label>
          <Group className="border-outline-variant bg-surface text-on-surface group-data-invalid:border-error focus-within:border-primary focus-within:ring-primary flex min-h-14 min-w-0 items-center gap-1 rounded-xl border px-2 focus-within:ring-1">
            <DateInput className="flex min-w-0 flex-1 flex-wrap items-center px-1 text-sm tabular-nums">
              {(segment) => (
                <DateSegment
                  segment={segment}
                  className="data-placeholder:text-on-surface-variant data-focused:bg-primary data-focused:text-on-primary rounded px-0.5 outline-none data-[type=literal]:px-0"
                />
              )}
            </DateInput>
            <Button
              ref={trigger}
              className={iconButtonClass}
              aria-label={m.date_time_open_calendar()}
            >
              <CalendarMonthRounded aria-hidden className="size-5" />
            </Button>
            <Button
              slot={null}
              className={iconButtonClass}
              aria-label={`${m.common_clear()}: ${label}`}
              isDisabled={!value}
              onPress={() => onChange('')}
            >
              <CloseRounded aria-hidden className="size-4" />
            </Button>
          </Group>
          <FieldError className="text-error text-xs" />
          <Popover
            UNSTABLE_portalContainer={container ?? undefined}
            data-slot="date-picker-popover"
            className="bg-surface text-on-surface border-outline-variant z-50 max-w-[calc(100vw-2rem)] rounded-3xl border p-4 shadow-lg"
            placement="bottom start"
            offset={8}
          >
            <Dialog className="outline-none">
              <Calendar
                className="w-72 max-w-full"
                onChange={(date) => {
                  onChange(
                    toCalendarDateTime(
                      date,
                      value ? parseDateTime(value) : undefined,
                    ).toString(),
                  )
                  setOpen(false)
                }}
              >
                <header className="mb-3 flex items-center gap-1">
                  <Heading className="flex-1 text-base font-medium" />
                  <Button slot="previous" className={iconButtonClass}>
                    <ChevronLeftRounded aria-hidden className="size-5" />
                  </Button>
                  <Button slot="next" className={iconButtonClass}>
                    <ChevronRightRounded aria-hidden className="size-5" />
                  </Button>
                </header>
                <CalendarGrid className="w-full border-separate border-spacing-y-1">
                  <CalendarGridHeader>
                    {(day) => (
                      <CalendarHeaderCell className="text-on-surface-variant h-8 text-xs font-medium">
                        {day}
                      </CalendarHeaderCell>
                    )}
                  </CalendarGridHeader>
                  <CalendarGridBody>
                    {(date) => (
                      <CalendarCell
                        date={date}
                        className="hover:bg-surface-variant/40 data-selected:bg-primary data-selected:text-on-primary data-today:border-primary focus-visible:ring-primary mx-auto grid size-9 cursor-pointer place-items-center rounded-full border border-transparent text-sm tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-offset-2 data-disabled:cursor-default data-disabled:opacity-40 data-outside-month:invisible data-today:border"
                      />
                    )}
                  </CalendarGridBody>
                </CalendarGrid>
              </Calendar>
            </Dialog>
          </Popover>
        </DatePicker>
      </div>
    </I18nProvider>
  )
}

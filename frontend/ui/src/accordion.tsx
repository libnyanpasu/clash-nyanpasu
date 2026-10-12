import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { Accordion as AccordionPrimitive } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

// Radix unmounts or hides closed content immediately, which would cut an exit
// animation short, so the open state is tracked here and the content is
// mounted by AnimatePresence instead. That is why only the single-open,
// collapsible form is offered.
const AccordionValueContext = createContext<string>('')

const AccordionItemContext = createContext<{ open: boolean }>({ open: false })

export type AccordionProps = Omit<
  ComponentProps<typeof AccordionPrimitive.Root>,
  'type' | 'collapsible' | 'value' | 'defaultValue' | 'onValueChange'
> & {
  /** The open item's value; an empty string means every item is collapsed. */
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
}

export function Accordion({
  value: controlledValue,
  defaultValue,
  onValueChange,
  className,
  ...props
}: AccordionProps) {
  const [value, setValue] = useControllableState({
    prop: controlledValue,
    defaultProp: defaultValue ?? '',
    onChange: onValueChange,
  })

  return (
    <AccordionValueContext.Provider value={value}>
      <AccordionPrimitive.Root
        data-slot="accordion"
        type="single"
        collapsible
        value={value}
        onValueChange={setValue}
        className={cn('flex flex-col gap-2', className)}
        {...props}
      />
    </AccordionValueContext.Provider>
  )
}

export function AccordionItem({
  value,
  className,
  ...props
}: ComponentProps<typeof AccordionPrimitive.Item>) {
  const open = useContext(AccordionValueContext) === value

  return (
    <AccordionItemContext.Provider value={{ open }}>
      <AccordionPrimitive.Item
        data-slot="accordion-item"
        value={value}
        className={cn(
          'bg-surface-variant/30 min-w-0 overflow-hidden rounded-3xl',
          'transition-colors motion-reduce:transition-none',
          'data-[state=open]:bg-surface-variant/50',
          className,
        )}
        {...props}
      />
    </AccordionItemContext.Provider>
  )
}

export function AccordionTrigger({
  className,
  children,
  ...props
}: ComponentProps<typeof AccordionPrimitive.Trigger>) {
  const { open } = useContext(AccordionItemContext)
  const reduceMotion = useReducedMotion()

  return (
    <AccordionPrimitive.Header data-slot="accordion-header" className="flex">
      <AccordionPrimitive.Trigger
        data-slot="accordion-trigger"
        className={cn(
          'flex min-h-16 flex-1 cursor-pointer items-center gap-3 px-4 py-3 text-left select-none',
          'hover:bg-on-surface/8 transition-colors motion-reduce:transition-none',
          'focus-visible:outline-primary outline-none focus-visible:outline-2 focus-visible:-outline-offset-2',
          'disabled:cursor-not-allowed disabled:opacity-38',
          className,
        )}
        {...props}
      >
        {children}

        <motion.span
          aria-hidden="true"
          data-slot="accordion-chevron"
          className="text-on-surface-variant inline-flex shrink-0"
          initial={false}
          animate={{ rotate: open ? 180 : 0 }}
          transition={
            reduceMotion
              ? { duration: 0 }
              : { duration: 0.2, ease: [0.2, 0, 0, 1] }
          }
        >
          <ExpandMoreRounded className="size-6" />
        </motion.span>
      </AccordionPrimitive.Trigger>
    </AccordionPrimitive.Header>
  )
}

export function AccordionContent({
  className,
  children,
  ...props
}: ComponentProps<typeof AccordionPrimitive.Content>) {
  const { open } = useContext(AccordionItemContext)
  const reduceMotion = useReducedMotion()

  const transition = reduceMotion
    ? { duration: 0 }
    : {
        height: { duration: 0.25, ease: [0.2, 0, 0, 1] as const },
        opacity: { duration: 0.15 },
      }

  return (
    <AnimatePresence initial={false}>
      {open && (
        <AccordionPrimitive.Content forceMount asChild {...props}>
          <motion.div
            data-slot="accordion-content"
            className="overflow-hidden"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={transition}
          >
            <div className={cn('flex flex-col gap-4 px-4 pb-4', className)}>
              {children}
            </div>
          </motion.div>
        </AccordionPrimitive.Content>
      )}
    </AnimatePresence>
  )
}

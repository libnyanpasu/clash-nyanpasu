import Check from '~icons/material-symbols/check-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/ui'
import * as DropdownMenuPrimitive from '@radix-ui/react-dropdown-menu'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const DropdownMenuContext = createContext<{
  open: boolean
} | null>(null)

const useDropdownMenuContext = () => {
  const context = useContext(DropdownMenuContext)

  if (context === null) {
    throw new Error(
      'DropdownMenu compound components cannot be rendered outside the DropdownMenu component',
    )
  }

  return context
}

export const DropdownMenu = ({
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Root>) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <DropdownMenuContext.Provider value={{ open }}>
      <DropdownMenuPrimitive.Root
        {...props}
        open={open}
        onOpenChange={setOpen}
      />
    </DropdownMenuContext.Provider>
  )
}

export const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger

export const DropdownMenuGroup = DropdownMenuPrimitive.Group

export const DropdownMenuPortal = DropdownMenuPrimitive.Portal

export const DropdownMenuSub = DropdownMenuPrimitive.Sub

const DropdownMenuRadioGroupContext = createContext<{
  value: string | null
}>({ value: null })

const useDropdownMenuRadioGroupContext = () => {
  const context = useContext(DropdownMenuRadioGroupContext)

  if (context === undefined) {
    throw new Error(
      'DropdownMenuRadioGroup compound components cannot be rendered outside the DropdownMenuRadioGroup component',
    )
  }

  return context
}

export const DropdownMenuRadioGroup = ({
  value: inputValue,
  defaultValue,
  onValueChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioGroup>) => {
  const [value, setValue] = useControllableState({
    prop: inputValue,
    defaultProp: String(defaultValue),
    onChange: onValueChange,
  })

  return (
    <DropdownMenuRadioGroupContext.Provider value={{ value }}>
      <DropdownMenuPrimitive.RadioGroup
        {...props}
        value={value}
        onValueChange={setValue}
      />
    </DropdownMenuRadioGroupContext.Provider>
  )
}

export const DropdownMenuSubTrigger = DropdownMenuPrimitive.SubTrigger

export const DropdownMenuSubContent = DropdownMenuPrimitive.SubContent

export const DropdownMenuContent = ({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) => {
  const { open } = useDropdownMenuContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DropdownMenuPrimitive.Portal forceMount>
          <DropdownMenuPrimitive.Content {...props} asChild>
            <motion.div
              className={cn(
                'shadow-container relative z-50 w-full overflow-auto rounded',
                'dark:text-on-surface',
                'bg-inverse-on-surface dark:bg-surface',
                className,
              )}
              style={{
                maxHeight: 'var(--radix-popper-available-height)',
              }}
              initial={{
                opacity: 0,
                scaleY: 0.9,
                transformOrigin: 'top',
              }}
              animate={{
                opacity: 1,
                scaleY: 1,
                transformOrigin: 'top',
              }}
              exit={{
                opacity: 0,
                scaleY: 0.9,
                transformOrigin: 'top',
              }}
              transition={{
                type: 'spring',
                bounce: 0,
                duration: 0.35,
              }}
            >
              {children}
            </motion.div>
          </DropdownMenuPrimitive.Content>
        </DropdownMenuPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export const DropdownMenuItem = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) => {
  return (
    <DropdownMenuPrimitive.Item
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        className,
      )}
      {...props}
    />
  )
}

export const DropdownMenuCheckboxItem = ({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.CheckboxItem>) => {
  return (
    <DropdownMenuPrimitive.CheckboxItem
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      {...props}
    >
      {children}

      <DropdownMenuPrimitive.ItemIndicator>
        <Check className="text-primary" />
      </DropdownMenuPrimitive.ItemIndicator>
    </DropdownMenuPrimitive.CheckboxItem>
  )
}

export const DropdownMenuRadioItem = ({
  value,
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioItem>) => {
  const context = useDropdownMenuRadioGroupContext()

  const selected = context.value === value

  return (
    <DropdownMenuPrimitive.RadioItem
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      value={value}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator>
        <RadioChecked className="text-primary" />
      </DropdownMenuPrimitive.ItemIndicator>

      {!selected && (
        <span>
          <Radio className="text-outline-variant dark:text-outline" />
        </span>
      )}

      <div className="flex-1">{children}</div>
    </DropdownMenuPrimitive.RadioItem>
  )
}

export const DropdownMenuLabel = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Label>) => {
  return (
    <DropdownMenuPrimitive.Label
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        className,
      )}
      {...props}
    />
  )
}

export const DropdownMenuSeparator = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Separator>) => {
  return (
    <DropdownMenuPrimitive.Separator
      className={cn('bg-outline-variant/50 h-px', className)}
      {...props}
    />
  )
}

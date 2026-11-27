import ArrowDropDown from '~icons/material-symbols/arrow-drop-down-rounded'
import Check from '~icons/material-symbols/check-rounded'
import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence, motion } from 'motion/react'
import { Select as SelectPrimitive } from 'radix-ui'
import {
  Children,
  ComponentProps,
  createContext,
  isValidElement,
  useCallback,
  useContext,
  useLayoutEffect,
  useState,
  type ReactNode,
} from 'react'
import { chains, cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { launcherMenuChipClassName } from './launcher-menu'

export const selectTriggerVariants = cva(
  [
    'group relative box-border inline-flex w-full flex-auto items-baseline',
    'cursor-pointer',
    'px-4 py-4 outline-hidden',
    // TODO: size variants, fix this
    'flex items-center justify-between h-14',
    'text-on-surface',
  ],
  {
    variants: {
      variant: {
        filled: 'rounded-t-extra-small bg-surface-container-highest',
        // outlined use selectValuePlaceholderFieldsetVariants
        outlined: 'h-16 pt-2 pb-0',
        // compact pill that sits at the end of a menu row
        chip: [launcherMenuChipClassName, 'gap-1'],
      },
    },
    defaultVariants: {
      variant: 'filled',
    },
  },
)

export type SelectTriggerVariants = VariantProps<typeof selectTriggerVariants>

export const selectLineVariants = cva('', {
  variants: {
    variant: {
      filled: [
        'absolute inset-x-0 bottom-0 w-full border-b border-outline-variant',
        'transition-all duration-200',

        // pseudo elements be overlay parent element, will not affect the box size
        'after:absolute after:inset-x-0 after:bottom-0 after:z-10',
        "after:scale-x-0 after:border-b-2 after:opacity-0 after:content-['']",
        'after:transition-all after:duration-200',
        'after:border-primary',

        // sync parent group state, state from radix-ui
        'group-data-[state=open]:border-b-0',
        'group-data-[state=open]:after:scale-x-100',
        'group-data-[state=open]:after:opacity-100',
        'peer-focus:border-b-0',
        'peer-focus:after:scale-x-100',
        'peer-focus:after:opacity-100',
      ],
      // hidden line for outlined variant
      outlined: 'hidden',
      chip: 'hidden',
    },
  },
  defaultVariants: {
    variant: 'filled',
  },
})

export type SelectLineVariants = VariantProps<typeof selectLineVariants>

export const selectValueVariants = cva(
  'pointer-events-none transition-[margin] duration-200',
  {
    variants: {
      variant: {
        filled: '',
        outlined: '',
        chip: 'truncate',
      },
      haveValue: {
        true: '',
        false: '',
      },
    },
    compoundVariants: [
      {
        variant: 'filled',
        haveValue: true,
        className: 'mt-3!',
      },
    ],
    defaultVariants: {
      variant: 'filled',
      haveValue: false,
    },
  },
)

export type SelectValueVariants = VariantProps<typeof selectValueVariants>

export const selectValuePlaceholderVariants = cva(
  [
    'absolute',
    'left-4 top-4',
    'pointer-events-none',
    'text-base select-none',
    // TODO: only transition position, not text color
    'transition-all duration-200',
  ],
  {
    variants: {
      variant: {
        filled: [
          'group-data-[state=open]:top-2',
          'group-data-[state=open]:text-xs group-data-[state=open]:text-primary',
        ],
        outlined: [
          'top-6',
          'group-data-[state=open]:top-0',
          'group-data-[state=open]:text-xs',
          'group-data-[state=open]:text-primary',
        ],
        chip: 'hidden',
      },
      focus: {
        true: '',
        false: '',
      },
    },
    compoundVariants: [
      {
        variant: 'filled',
        focus: true,
        className: 'top-2 text-xs',
      },
      {
        variant: 'outlined',
        focus: true,
        className: 'top-0 text-xs',
      },
    ],
    defaultVariants: {
      variant: 'filled',
      focus: false,
    },
  },
)

export type SelectValuePlaceholderVariants = VariantProps<
  typeof selectValuePlaceholderVariants
>

export const selectValuePlaceholderFieldsetVariants = cva(
  'pointer-events-none',
  {
    variants: {
      variant: {
        // only for outlined variant
        filled: 'hidden',
        outlined: [
          'absolute inset-x-0 top-2 bottom-0 text-left',
          'rounded-extra-small transition-all duration-200',
          // may open border width will be 1.5, idk
          'group-data-[state=closed]:border',
          'group-data-[state=open]:border-2',
          'peer-not-focus:border',
          'peer-focus:border-2',
          // different material web border color, i think this looks better
          'group-data-[state=closed]:border-outline-variant',
          'group-data-[state=open]:border-primary',
          'peer-not-focus:border-outline-variant',
          'peer-focus:border-primary',
        ],
        chip: 'hidden',
      },
    },
    defaultVariants: {
      variant: 'filled',
    },
  },
)

export type SelectValuePlaceholderFieldsetVariants = VariantProps<
  typeof selectValuePlaceholderFieldsetVariants
>

export const selectValuePlaceholderLegendVariants = cva('', {
  variants: {
    variant: {
      // only for outlined variant
      filled: 'hidden',
      outlined: 'invisible ml-2 px-2 text-xs h-0',
      chip: 'hidden',
    },
    haveValue: {
      true: '',
      false: '',
    },
  },
  compoundVariants: [
    {
      variant: 'outlined',
      haveValue: false,
      className: 'group-data-[state=closed]:hidden group-not-focus:hidden',
    },
  ],
  defaultVariants: {
    variant: 'filled',
    haveValue: false,
  },
})

export type SelectValuePlaceholderLegendVariants = VariantProps<
  typeof selectValuePlaceholderLegendVariants
>

export const selectContentVariants = cva(
  [
    'relative w-full overflow-auto rounded-extra-small z-50',
    'bg-surface-container-high text-on-surface',
    'shadow-sm shadow-shadow/15',
    'border border-outline-variant/50',
  ],
  {
    variants: {
      variant: {
        filled: 'rounded-t-none',
        outlined: '',
        chip: 'min-w-40 rounded-large',
      },
    },
    defaultVariants: {
      variant: 'filled',
    },
  },
)

export type SelectContentVariants = VariantProps<typeof selectContentVariants>

type SelectContextType = {
  haveValue?: boolean
  open?: boolean
  resolvedLabel?: ReactNode
  registerLabels: (next: Map<string, ReactNode>) => void
} & SelectTriggerVariants

function collectSelectItemLabels(
  nodes: ReactNode,
  labels: Map<string, ReactNode>,
) {
  Children.forEach(nodes, (child) => {
    if (!isValidElement<{ value?: unknown; children?: ReactNode }>(child)) {
      return
    }

    if (
      (child.type === SelectItem || child.type === SelectPrimitive.Item) &&
      typeof child.props.value === 'string'
    ) {
      labels.set(child.props.value, child.props.children)
      return
    }

    if (child.props.children !== undefined) {
      collectSelectItemLabels(child.props.children, labels)
    }
  })
}

function mapsEqual(a: Map<string, ReactNode>, b: Map<string, ReactNode>) {
  if (a.size !== b.size) return false
  for (const [key, value] of a) {
    if (!b.has(key) || b.get(key) !== value) return false
  }
  return true
}

const SelectContext = createContext<SelectContextType | null>(null)

const useSelectContext = () => {
  const context = useContext(SelectContext)

  if (!context) {
    throw new Error('useSelectContext must be used within a SelectProvider')
  }

  return context
}

export const SelectLine = ({ className, ...props }: ComponentProps<'div'>) => {
  const { variant } = useSelectContext()

  return (
    <div
      className={cn(
        selectLineVariants({
          variant,
        }),
        className,
      )}
      {...props}
    />
  )
}

export const Select = ({
  value: inputValue,
  defaultValue,
  onValueChange,
  variant,
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: React.ComponentProps<typeof SelectPrimitive.Root> &
  SelectTriggerVariants) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  const [value, setValue] = useControllableState({
    prop: inputValue,
    defaultProp: defaultValue ?? '',
    onChange: onValueChange,
  })

  const [itemLabels, setItemLabels] = useState<Map<string, ReactNode>>(
    () => new Map(),
  )

  const registerLabels = useCallback((next: Map<string, ReactNode>) => {
    setItemLabels((current) => (mapsEqual(current, next) ? current : next))
  }, [])

  const haveValue = Boolean(value)
  const resolvedLabel = haveValue ? itemLabels.get(value) : undefined

  return (
    <SelectContext.Provider
      value={{
        open,
        haveValue,
        variant,
        resolvedLabel,
        registerLabels,
      }}
    >
      <SelectPrimitive.Root
        open={open}
        onOpenChange={setOpen}
        value={inputValue !== undefined ? value : undefined}
        defaultValue={inputValue === undefined ? defaultValue : undefined}
        onValueChange={chains(setValue)}
        {...props}
      />
    </SelectContext.Provider>
  )
}

export type SelectProps = ComponentProps<typeof Select>

export const SelectValue = ({
  className,
  placeholder,
  children,
  ...props
}: ComponentProps<typeof SelectPrimitive.Value>) => {
  const { haveValue, open, variant, resolvedLabel } = useSelectContext()

  return (
    <>
      <div
        className={cn(
          selectValueVariants({
            variant,
            haveValue,
          }),
          className,
        )}
      >
        <SelectPrimitive.Value {...props}>
          {children ?? resolvedLabel}
        </SelectPrimitive.Value>
      </div>

      <fieldset
        className={cn(
          selectValuePlaceholderFieldsetVariants({
            variant,
          }),
        )}
      >
        {/* an empty legend would still cut a notch into the outlined border */}
        {placeholder && (
          <legend
            className={cn(
              selectValuePlaceholderLegendVariants({
                variant,
                haveValue: haveValue || open,
              }),
            )}
          >
            {placeholder}
          </legend>
        )}
      </fieldset>

      <div
        className={cn(
          selectValuePlaceholderVariants({
            variant,
            focus: haveValue || open,
          }),
        )}
      >
        {placeholder}
      </div>
    </>
  )
}

export const SelectGroup = (
  props: ComponentProps<typeof SelectPrimitive.Group>,
) => {
  return <SelectPrimitive.Group {...props} />
}

export const SelectLabel = ({
  className,
  ...props
}: ComponentProps<typeof SelectPrimitive.Label>) => {
  return (
    <SelectPrimitive.Label
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'text-primary',
        className,
      )}
      {...props}
    />
  )
}

export const SelectTrigger = ({
  className,
  children,
  ...props
}: ComponentProps<typeof SelectPrimitive.Trigger>) => {
  const { variant } = useSelectContext()

  return (
    <SelectPrimitive.Trigger
      className={cn(
        selectTriggerVariants({
          variant,
        }),
        className,
      )}
      {...props}
    >
      {children}

      <SelectLine />

      <SelectIcon />
    </SelectPrimitive.Trigger>
  )
}

export const SelectIcon = ({
  asChild,
  children,
  className,
  ...props
}: ComponentProps<typeof SelectPrimitive.Icon>) => {
  const { variant } = useSelectContext()

  return (
    <SelectPrimitive.Icon
      className={cn(
        variant === 'chip' ? '-mr-1 size-5 shrink-0' : 'absolute right-4',
        className,
      )}
      asChild
      {...props}
    >
      {asChild ? children : <ArrowDropDown />}
    </SelectPrimitive.Icon>
  )
}

export const SelectContent = ({
  className,
  children,
  ...props
}: ComponentProps<typeof SelectPrimitive.Content>) => {
  const { open, variant, registerLabels } = useSelectContext()

  // Keep SelectValue populated even while AnimatePresence unmounts the portal
  // when closed so Radix does not clear the displayed label between opens.
  useLayoutEffect(() => {
    const labels = new Map<string, ReactNode>()
    collectSelectItemLabels(children, labels)
    registerLabels(labels)
  }, [children, registerLabels])

  return (
    <AnimatePresence initial={false}>
      {open && (
        <SelectPrimitive.Portal>
          <SelectPrimitive.Content {...props} position="popper" asChild>
            <motion.div
              className={cn(
                selectContentVariants({
                  variant,
                }),
                className,
              )}
              style={{
                width:
                  variant === 'chip'
                    ? undefined
                    : 'var(--radix-popper-anchor-width)',
                maxHeight: 'var(--radix-popper-available-height)',
              }}
              initial={{
                opacity: 0,
                scaleY: 0.9,
                transformOrigin: 'var(--radix-popper-transform-origin)',
              }}
              animate={{
                opacity: 1,
                scaleY: 1,
                transformOrigin: 'var(--radix-popper-transform-origin)',
              }}
              exit={{
                opacity: 0,
                scaleY: 0.9,
                transformOrigin: 'var(--radix-popper-transform-origin)',
              }}
              transition={{
                type: 'spring',
                bounce: 0,
                duration: 0.35,
              }}
            >
              <SelectPrimitive.Viewport>{children}</SelectPrimitive.Viewport>
            </motion.div>
          </SelectPrimitive.Content>
        </SelectPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export const SelectItem = ({
  className,
  children,
  ...props
}: ComponentProps<typeof SelectPrimitive.Item>) => {
  return (
    <SelectPrimitive.Item
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
        'data-[state=checked]:bg-primary-container data-[state=checked]:text-on-primary-container',
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>

      <SelectPrimitive.ItemIndicator>
        <Check className="text-on-primary-container" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  )
}

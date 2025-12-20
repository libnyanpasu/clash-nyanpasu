import ArrowDropDown from '~icons/material-symbols/arrow-drop-down-rounded'
import Check from '~icons/material-symbols/check-rounded'
import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence, motion } from 'framer-motion'
import {
  ComponentProps,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from 'react'
import { chains } from '@/utils/chain'
import { cn } from '@nyanpasu/ui'
import * as SelectPrimitive from '@radix-ui/react-select'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

export const selectTriggerVariants = cva(
  [
    'group relative box-border inline-flex w-full flex-auto items-baseline',
    'cursor-pointer',
    'px-4 py-4 outline-hidden',
    // TODO: size variants, fix this
    'flex items-center justify-between h-14',
    'dark:text-on-surface',
  ],
  {
    variants: {
      variant: {
        filled: 'rounded-t bg-surface-variant/30 dark:bg-surface',
        // outlined use selectValuePlaceholderFieldsetVariants
        outlined: '',
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
        'absolute inset-x-0 bottom-0 w-full border-b border-on-primary-container',
        'transition-all duration-200',

        // pseudo elements be overlay parent element, will not affect the box size
        'after:absolute after:inset-x-0 after:bottom-0 after:z-10',
        "after:scale-x-0 after:border-b-2 after:opacity-0 after:content-['']",
        'after:transition-all after:duration-200',
        'after:border-primary dark:after:border-on-primary-container',

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
          'group-data-[state=open]:-top-2',
          'group-data-[state=open]:text-sm',
          'group-data-[state=open]:text-primary',

          'dark:group-data-[state=open]:text-inverse-primary',
          'dark:group-data-[state=closed]:text-on-primary-container',
        ],
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
        className: '-top-2 text-sm',
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
          'absolute inset-0 text-left',
          'rounded transition-all duration-200',
          // may open border width will be 1.5, idk
          'group-data-[state=closed]:border',
          'group-data-[state=open]:border-2',
          'peer-not-focus:border',
          'peer-focus:border-2',
          // different material web border color, i think this looks better
          'group-data-[state=closed]:border-outline-variant',
          'group-data-[state=open]:border-primary',
          'peer-not-focus:border-primary-container',
          'peer-focus:border-primary',
          // dark must be prefixed
          'dark:group-data-[state=closed]:border-outline-variant',
          'dark:group-data-[state=open]:border-primary-container',
          'dark:peer-not-focus:border-outline-variant',
          'dark:peer-focus:border-primary-container',
        ],
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
      outlined: 'invisible ml-2 px-2 text-sm h-0',
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
    'relative w-full overflow-auto rounded shadow-container z-50',
    'bg-inverse-on-surface dark:bg-surface',
    'dark:text-on-surface',
  ],
  {
    variants: {
      variant: {
        filled: 'rounded-t-none',
        outlined: '',
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
} & SelectTriggerVariants

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

  const [haveValue, setHaveValue] = useState(
    Boolean(props.value || props.defaultValue),
  )

  const handleOnChange = useCallback((value?: string) => {
    setHaveValue(Boolean(value))
  }, [])

  useEffect(() => {
    setHaveValue(Boolean(props.value || props.defaultValue))
  }, [props.value, props.defaultValue])

  return (
    <SelectContext.Provider
      value={{
        open,
        haveValue,
        variant,
      }}
    >
      <SelectPrimitive.Root
        open={open}
        onOpenChange={setOpen}
        onValueChange={chains(handleOnChange, onValueChange)}
        {...props}
      />
    </SelectContext.Provider>
  )
}

export type SelectProps = ComponentProps<typeof Select>

export const SelectValue = ({
  className,
  placeholder,
  ...props
}: ComponentProps<typeof SelectPrimitive.Value>) => {
  const { haveValue, open, variant } = useSelectContext()

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
        <SelectPrimitive.Value {...props} />
      </div>

      <fieldset
        className={cn(
          selectValuePlaceholderFieldsetVariants({
            variant,
          }),
        )}
      >
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
        'text-primary dark:text-inverse-primary',
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
  return (
    <SelectPrimitive.Icon
      className={cn('absolute right-4', className)}
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
  const { open, variant } = useSelectContext()

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
                width: 'var(--radix-popper-anchor-width)',
                maxHeight: 'var(--radix-popper-available-height)',
              }}
              initial={{ opacity: 0, scaleY: 0.9, transformOrigin: 'top' }}
              animate={{ opacity: 1, scaleY: 1, transformOrigin: 'top' }}
              exit={{ opacity: 0, scaleY: 0.9, transformOrigin: 'top' }}
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
        'hover:bg-surface-variant data-[state=checked]:bg-primary-container',
        'dark:hover:bg-surface-variant dark:data-[state=checked]:bg-primary-container',
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>

      <SelectPrimitive.ItemIndicator>
        <Check className="text-primary" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  )
}

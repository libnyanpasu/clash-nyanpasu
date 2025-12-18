import { useCreation } from 'ahooks'
import { cva, type VariantProps } from 'class-variance-authority'
import React, { useEffect } from 'react'
import { cn } from '@nyanpasu/ui'

export const inputContainerVariants = cva(
  [
    'group relative box-border inline-flex w-full flex-auto items-baseline',
    'cursor-pointer',
    'px-4 py-4 outline-hidden',
    // TODO: size variants, fix this
    'flex items-center justify-between h-14',
    'dark:text-surface',
  ],
  {
    variants: {
      variant: {
        filled: ['rounded-t', 'bg-surface-variant dark:bg-on-surface-variant'],
        // outlined use selectValuePlaceholderFieldsetVariants
        outlined: '',
      },
    },
    defaultVariants: {
      variant: 'filled',
    },
  },
)

export type InputContainerVariants = VariantProps<typeof inputContainerVariants>

export const inputVariants = cva(
  [
    'peer',
    'w-full border-none p-0',
    'bg-transparent placeholder-transparent outline-hidden',
  ],
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
      haveLabel: {
        true: '',
        false: '',
      },
    },
    compoundVariants: [
      {
        variant: 'filled',
        haveValue: true,
        haveLabel: true,
        className: 'mt-3',
      },
    ],
    defaultVariants: {
      variant: 'filled',
      haveValue: false,
      haveLabel: false,
    },
  },
)

export type InputVariants = VariantProps<typeof inputVariants>

export const inputLabelVariants = cva(
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
          'group-data-[state=open]:top-2 group-data-[state=open]:dark:text-surface',
          'group-data-[state=open]:text-xs group-data-[state=open]:text-primary',
        ],
        outlined: [
          'group-data-[state=open]:-top-2',
          'group-data-[state=open]:text-sm',
          'group-data-[state=open]:text-primary',

          'dark:group-data-[state=open]:text-inverse-primary',
          'dark:group-data-[state=closed]:text-primary-container',

          // "before:absolute before:inset-0 before:content-['']",
          // "before:-z-10 before:-mx-1",
          // "before:bg-transparent ",
          // "before:inline-block",
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

export type InputLabelVariants = VariantProps<typeof inputLabelVariants>

export const inputLineVariants = cva('', {
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

export type InputLineVariants = VariantProps<typeof inputLineVariants>

export const inputLabelFieldsetVariants = cva('pointer-events-none', {
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
        'group-data-[state=closed]:border-primary-container',
        'group-data-[state=open]:border-primary',
        'peer-not-focus:border-primary-container',
        'peer-focus:border-primary',
        // dark must be prefixed
        'dark:group-data-[state=closed]:border-primary-container',
        'dark:group-data-[state=open]:border-inverse-primary',
        'dark:peer-not-focus:border-primary-container',
        'dark:peer-focus:border-inverse-primary',
      ],
    },
  },
  defaultVariants: {
    variant: 'filled',
  },
})

export type InputLabelFieldsetVariants = VariantProps<
  typeof inputLabelFieldsetVariants
>

export const inputLabelLegendVariants = cva('', {
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
      className: ['group-data-[state=closed]:hidden', 'group-not-focus:hidden'],
    },
  ],
  defaultVariants: {
    variant: 'filled',
    haveValue: false,
  },
})

export type InputLabelLegendVariants = VariantProps<
  typeof inputLabelLegendVariants
>

type InputContextType = {
  haveLabel?: boolean
  haveValue?: boolean
} & InputContainerVariants

const InputContext = React.createContext<InputContextType | null>(null)

const useInputContext = () => {
  const context = React.useContext(InputContext)

  if (!context) {
    throw new Error('InputContext is undefined')
  }

  return context
}

export const InputContainer = ({
  className,
  ...props
}: React.ComponentProps<'div'>) => {
  const { variant } = useInputContext()

  return (
    <div
      className={cn(
        inputContainerVariants({
          variant,
        }),
        className,
      )}
      {...props}
    />
  )
}

export const InputLine = ({
  className,
  ...props
}: React.ComponentProps<'input'>) => {
  const { variant } = useInputContext()

  return (
    <div
      className={cn(
        inputLineVariants({
          variant,
        }),
        className,
      )}
      {...props}
    />
  )
}

export type InputProps = React.ComponentProps<'input'> & {
  label?: string
} & InputContainerVariants

export const Input = ({
  variant,
  className,
  label,
  children,
  onChange,
  ...props
}: InputProps) => {
  const [haveValue, setHaveValue] = React.useState(false)

  const haveLabel = useCreation(() => {
    if (label) {
      return true
    }

    if (React.isValidElement(children)) {
      if (typeof children.type !== 'string') {
        if ('displayName' in children.type) {
          if (children.type.displayName === InputLabel.displayName) {
            return true
          }
        }
      }
    }

    return false
  }, [])

  useEffect(() => {
    if (props.value || props.defaultValue) {
      setHaveValue(true)
    } else {
      setHaveValue(false)
    }
  }, [props.value, props.defaultValue])

  const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setHaveValue(event.target.value.length > 0)
    onChange?.(event)
  }

  useEffect(() => {
    console.log('haveValue', haveValue)
  }, [haveValue])

  return (
    <InputContext.Provider
      value={{
        haveLabel,
        haveValue,
        variant,
      }}
    >
      <InputContainer>
        <input
          className={cn(
            inputVariants({
              variant,
              haveValue,
              haveLabel,
            }),
            className,
          )}
          onChange={handleChange}
          {...props}
        />

        {label && (
          <>
            <fieldset
              className={cn(
                inputLabelFieldsetVariants({
                  variant,
                }),
              )}
            >
              <legend
                className={cn(
                  inputLabelLegendVariants({
                    variant,
                    haveValue,
                  }),
                )}
              >
                {label}
              </legend>
            </fieldset>

            <InputLabel>{label}</InputLabel>
          </>
        )}

        {children}

        <InputLine />
      </InputContainer>
    </InputContext.Provider>
  )
}

Input.displayName = 'Input'

export const InputLabel = ({
  className,
  ...props
}: React.ComponentProps<'label'>) => {
  const { haveValue, variant } = useInputContext()

  return (
    <label
      className={cn(
        inputLabelVariants({
          variant,
          focus: haveValue,
        }),
      )}
      {...props}
    />
  )
}

InputLabel.displayName = 'InputLabel'

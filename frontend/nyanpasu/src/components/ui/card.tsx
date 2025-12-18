import { cva, type VariantProps } from 'class-variance-authority'
import { createContext, HTMLAttributes, useContext } from 'react'
import { cn } from '@nyanpasu/ui'
import { Slot } from '@radix-ui/react-slot'

export const cardVariants = cva('rounded-3xl text-on-surface overflow-hidden', {
  variants: {
    variant: {
      basic: ['shadow-sm', 'bg-surface dark:bg-surface'],
      raised: ['shadow-sm', 'bg-primary-container dark:bg-on-primary'],
      outline: [
        'bg-surface dark:bg-surface',
        'border border-outline-variant dark:border-outline-variant',
      ],
    },
  },
  defaultVariants: {
    variant: 'basic',
  },
})

export type CardVariantsProps = VariantProps<typeof cardVariants>

export const cardContentVariants = cva(['flex flex-col gap-4 p-4'])

export type CardContentVariantsProps = VariantProps<typeof cardContentVariants>

export const cardHeaderVariants = cva(
  ['flex items-center gap-4 text-xl', 'px-4'],
  {
    variants: {
      variant: {
        basic: 'border-surface-variant dark:border-surface-variant',
        raised: 'border-inverse-primary dark:border-primary-container',
        outline: 'border-outline-variant dark:border-outline-variant',
      },
      divider: {
        true: 'border-b py-4 ',
        false: 'pt-4',
      },
    },
    defaultVariants: {
      divider: false,
      variant: 'basic',
    },
  },
)

export type CardHeaderVariantsProps = VariantProps<typeof cardHeaderVariants>

export const cardFooterVariants = cva(
  ['flex flex-row-reverse items-center gap-4', 'px-2'],
  {
    variants: {
      variant: {
        basic: 'border-surface-variant dark:border-surface-variant',
        raised: 'border-inverse-primary dark:border-primary-container',
        outline: 'border-outline-variant dark:border-outline-variant',
      },
      divider: {
        true: 'border-t py-2',
        false: 'pb-2',
      },
    },
    defaultVariants: {
      divider: false,
      variant: 'basic',
    },
  },
)

export type CardFooterVariantsProps = VariantProps<typeof cardFooterVariants>

type CardContextType = {
  variant: CardVariantsProps['variant']
  divider: CardHeaderVariantsProps['divider'] &
    CardFooterVariantsProps['divider']
}

const CardContext = createContext<CardContextType | null>(null)

const useCardContext = () => {
  const context = useContext(CardContext)

  if (!context) {
    throw new Error('useCardContext must be used within a CardProvider')
  }

  return context
}

export interface CardProps
  extends
    HTMLAttributes<HTMLDivElement>,
    CardVariantsProps,
    Partial<CardContextType> {
  asChild?: boolean
}

export const Card = ({
  variant,
  divider,
  asChild,
  className,
  ...props
}: CardProps) => {
  const Comp = asChild ? Slot : 'div'

  return (
    <CardContext.Provider
      value={{
        variant,
        divider,
      }}
    >
      <Comp
        className={cn(
          cardVariants({
            variant,
          }),
          className,
        )}
        {...props}
      />
    </CardContext.Provider>
  )
}

export type CardContentProps = HTMLAttributes<HTMLDivElement> &
  CardContentVariantsProps

export const CardContent = ({ className, ...props }: CardContentProps) => {
  return <div className={cn(cardContentVariants(), className)} {...props} />
}

export type CardHeaderProps = HTMLAttributes<HTMLDivElement> &
  CardHeaderVariantsProps & {
    asChild?: boolean
  }

export const CardHeader = ({
  divider,
  variant,
  className,
  ...props
}: CardHeaderProps) => {
  const context = useCardContext()

  return (
    <div
      className={cn(
        cardHeaderVariants({
          divider: context?.divider ?? divider,
          variant: context?.variant ?? variant,
        }),
        className,
      )}
      {...props}
    />
  )
}

export interface CardFooterProps
  extends HTMLAttributes<HTMLDivElement>, CardFooterVariantsProps {}

export const CardFooter = ({
  divider,
  variant,
  className,
  ...props
}: CardFooterProps) => {
  const context = useCardContext()

  return (
    <div
      className={cn(
        cardFooterVariants({
          divider: context?.divider ?? divider,
          variant: context?.variant ?? variant,
        }),
        className,
      )}
      {...props}
    />
  )
}

import { cva, type VariantProps } from 'class-variance-authority'
import type { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { cn } from '@nyanpasu/utils'

const compactButtonVariants = cva(
  [
    'inline-flex h-8 min-w-0 items-center justify-center gap-2 px-3.5 text-[13px] whitespace-nowrap',
    // The design dims a disabled button as a whole, whatever its tone.
    'disabled:opacity-40',
    '[&_svg]:size-[18px]',
  ],
  {
    variants: {
      tone: {
        text: 'px-3 disabled:text-primary',
        tonal: [
          'bg-secondary-container text-on-secondary-container',
          'dark:bg-secondary-container dark:text-on-secondary-container',
          'hover:bg-secondary-container hover:brightness-95 dark:hover:bg-secondary-container dark:hover:brightness-110',
          'disabled:bg-secondary-container disabled:text-on-secondary-container',
        ],
        filled: [
          'bg-primary text-on-primary dark:bg-primary dark:text-on-primary',
          'hover:bg-primary hover:brightness-110 dark:hover:bg-primary',
          'disabled:bg-primary disabled:text-on-primary dark:disabled:bg-primary',
        ],
      },
    },
    defaultVariants: { tone: 'text' },
  },
)

/**
 * The 32px button of a toolbar or a panel head: a text, tonal or filled pill.
 * It is the compact scale of the shared `Button`, not another button.
 */
export function CompactButton({
  tone,
  className,
  ...props
}: ComponentProps<typeof Button> & VariantProps<typeof compactButtonVariants>) {
  return (
    <Button
      type="button"
      variant={tone === 'filled' ? 'flat' : 'basic'}
      className={cn(compactButtonVariants({ tone }), className)}
      {...props}
    />
  )
}

/** An icon button at the end of a row; the label is also its tooltip. */
export function RowAction({
  label,
  className,
  children,
  ...props
}: ComponentProps<typeof Button> & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          icon
          aria-label={label}
          className={cn(
            'text-on-surface-variant dark:text-on-surface-variant size-9 shrink-0',
            'hover:bg-on-surface/8 dark:hover:bg-on-surface/8',
            className,
          )}
          {...props}
        >
          {children}
        </Button>
      </TooltipTrigger>

      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

/** Turns a `SearchField` into the design's outlined 40px search pill. */
export const searchPillClassName = cn(
  'bg-surface dark:bg-surface text-[13px]',
  'shadow-[inset_0_0_0_1px_var(--color-outline-variant)]',
  '[&_input]:text-[13px] [&>svg]:size-[18px]',
)

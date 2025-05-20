import { ReactNode } from 'react'
import { alpha, cn } from '@/utils'
import { Button, ButtonProps } from '@mui/material'

export interface FloatingButtonProps extends ButtonProps {
  children: ReactNode
  className?: string
}

export const FloatingButton = ({
  children,
  className,
  ...props
}: FloatingButtonProps) => {
  return (
    <Button
      className={cn(
        `right-8 bottom-8 z-10 size-16 !rounded-2xl backdrop-blur`,
        className,
      )}
      sx={(theme) => ({
        position: 'fixed',
        boxShadow: 8,
        backgroundColor: alpha(theme.vars.palette.primary.main, 0.3),

        '&:hover': {
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.45),
        },
      })}
      {...props}
    >
      {children}
    </Button>
  )
}

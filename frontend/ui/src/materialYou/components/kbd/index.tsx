import { cn } from '@/utils'
import { useTheme } from '@mui/material'
import styles from './index.module.scss'

export type Props = React.DetailedHTMLProps<
  React.HTMLAttributes<HTMLElement>,
  HTMLElement
>

export function Kbd({ className, children, ...rest }: Props) {
  const theme = useTheme()
  return (
    <kbd
      className={cn(
        styles.kbd,
        theme.palette.mode === 'dark' && styles.dark,
        className,
      )}
      {...rest}
    >
      {children}
    </kbd>
  )
}

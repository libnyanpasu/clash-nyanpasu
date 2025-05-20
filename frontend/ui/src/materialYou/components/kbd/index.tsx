import { cn } from '@/utils'
import { useColorScheme } from '@mui/material'
import styles from './index.module.scss'

export type Props = React.DetailedHTMLProps<
  React.HTMLAttributes<HTMLElement>,
  HTMLElement
>

export function Kbd({ className, children, ...rest }: Props) {
  const { mode } = useColorScheme()
  return (
    <kbd
      className={cn(styles.kbd, mode === 'dark' && styles.dark, className)}
      {...rest}
    >
      {children}
    </kbd>
  )
}

import { useControllableValue } from 'ahooks'
import { MouseEventHandler } from 'react'
import {
  Button as MuiButton,
  ButtonProps as MuiButtonProps,
} from '@mui/material'

export interface LoadingButtonProps extends Omit<MuiButtonProps, 'onClick'> {
  onClick?: MouseEventHandler<HTMLButtonElement>
}

export const LoadingButton = ({
  loading,
  onClick,
  ...props
}: LoadingButtonProps) => {
  const [pending, setPending] = useControllableValue<boolean>(
    { loading },
    {
      defaultValue: false,
    },
  )

  const handleClick: MouseEventHandler<HTMLButtonElement> = async (e) => {
    if (onClick) {
      setPending(true)
      try {
        await onClick(e)
      } catch (error) {
        console.error(error)
      } finally {
        setPending(false)
      }
    }
  }

  return <MuiButton {...props} onClick={handleClick} loading={pending} />
}

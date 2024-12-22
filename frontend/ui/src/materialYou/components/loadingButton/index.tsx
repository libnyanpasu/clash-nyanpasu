import { useControllableValue } from 'ahooks'
import { MouseEventHandler } from 'react'
import MuiLoadingButton, {
  LoadingButtonProps as MuiLoadingButtonProps,
} from '@mui/lab/LoadingButton'

export interface LoadingButtonProps
  extends Omit<MuiLoadingButtonProps, 'onClick'> {
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

  return <MuiLoadingButton {...props} onClick={handleClick} loading={pending} />
}

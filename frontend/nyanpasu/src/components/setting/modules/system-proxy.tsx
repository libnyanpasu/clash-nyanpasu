import { useControllableValue } from 'ahooks'
import { merge } from 'lodash-es'
import { memo, ReactNode } from 'react'
import { alpha, CircularProgress, SxProps, useTheme } from '@mui/material'
import { PaperButton, PaperButtonProps } from './nyanpasu-path'

export interface PaperSwitchButtonProps extends PaperButtonProps {
  label?: string
  checked: boolean
  loading?: boolean
  disableLoading?: boolean
  children?: ReactNode
  onClick?: () => Promise<void> | void
  sxPaper?: SxProps
}

export const PaperSwitchButton = memo(function PaperSwitchButton({
  label,
  checked,
  loading,
  disableLoading,
  children,
  onClick,
  sxPaper,
  ...props
}: PaperSwitchButtonProps) {
  const { palette } = useTheme()

  const [pending, setPending] = useControllableValue<boolean>(
    { loading },
    {
      defaultValue: false,
    },
  )

  const handleClick = async () => {
    if (onClick) {
      if (disableLoading) {
        return onClick()
      }

      setPending(true)
      await onClick()
      setPending(false)
    }
  }

  return (
    <PaperButton
      label={label}
      sxPaper={merge(
        {
          backgroundColor: checked
            ? alpha(palette.primary.main, 0.1)
            : palette.mode === 'dark'
              ? palette.common.black
              : palette.grey[100],
          cursor: pending ? 'progress' : 'none',
        },
        sxPaper,
      )}
      sxButton={{
        flexDirection: 'column',
        alignItems: 'start',
        gap: 0.5,
      }}
      onClick={handleClick}
      {...props}
    >
      {pending === true && (
        <CircularProgress
          sx={{
            position: 'absolute',
            bottom: 'calc(50% - 12px)',
            right: 12,
          }}
          color="inherit"
          size={24}
        />
      )}

      {children}
    </PaperButton>
  )
})

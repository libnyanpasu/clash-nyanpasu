import { InboxRounded } from '@mui/icons-material'
import { Box, Typography } from '@mui/material'
import { alpha } from '@nyanpasu/ui'

interface Props {
  text?: React.ReactNode
  extra?: React.ReactNode
}

export const BaseEmpty = (props: Props) => {
  const { text = 'Empty', extra } = props

  return (
    <Box
      sx={(theme) => ({
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        color: alpha(theme.vars.palette.text.secondary, 0.75),
      })}
    >
      <InboxRounded sx={{ fontSize: '4em' }} />
      <Typography sx={{ fontSize: '1.25em' }}>{text}</Typography>
      {extra}
    </Box>
  )
}

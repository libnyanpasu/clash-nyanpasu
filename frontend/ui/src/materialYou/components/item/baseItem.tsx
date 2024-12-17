import { FC, memo, ReactNode } from 'react'
import { SxProps } from '@mui/material'
import ListItem from '@mui/material/ListItem'
import ListItemText from '@mui/material/ListItemText'

export interface BaseItemProps {
  title: ReactNode
  children: ReactNode
  sxItem?: SxProps
  sxItemText?: SxProps
}

export const BaseItem: FC<BaseItemProps> = memo(function BaseItem({
  title,
  children,
  sxItem,
  sxItemText,
}: BaseItemProps) {
  return (
    <ListItem sx={{ pl: 0, pr: 0, ...sxItem }}>
      <ListItemText primary={title} sx={sxItemText} />

      {children}
    </ListItem>
  )
})

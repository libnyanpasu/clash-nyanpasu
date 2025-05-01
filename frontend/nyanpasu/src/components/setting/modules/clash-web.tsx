import { ReactElement, ReactNode } from 'react'
import Marquee from 'react-fast-marquee'
import DeleteRounded from '@mui/icons-material/DeleteRounded'
import EditRounded from '@mui/icons-material/EditRounded'
import OpenInNewRounded from '@mui/icons-material/OpenInNewRounded'
import Box from '@mui/material/Box'
import Chip from '@mui/material/Chip'
import IconButton from '@mui/material/IconButton'
import Paper, { PaperProps } from '@mui/material/Paper'
import { alpha, styled } from '@mui/material/styles'
import Typography from '@mui/material/Typography'
import { openThat } from '@nyanpasu/interface'

/**
 * @example
 * renderChip("http://localhost?server=%host", labels)
 *
 * @returns { (string | JSX.Element)[] }
 * (string | JSX.Element)[]
 *
 * `replace key string to Mui Chip.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const renderChip = (
  string: string,
  labels: {
    [label: string]: string | number | undefined | null
  },
): (string | ReactElement)[] => {
  return string.split(/(%[^&?]+)/).map((part, index) => {
    if (part.startsWith('%')) {
      const label = labels[part.replace('%', '')]

      // TODO: may should return part string
      if (!label) {
        return ''
      }

      return (
        <Chip
          sx={{
            '& .MuiChip-label': {
              pl: 0.5,
              pr: 0.5,
            },
          }}
          key={index}
          size="small"
          label={label}
        />
      )
    } else {
      return part
    }
  })
}

/**
 * @example
 * extractServer("127.0.0.1:7789")
 *
 * @returns { { host: string; port: number } }
 * { host: "127.0.0.1"; port: 7789 }
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const extractServer = (
  string?: string,
): { host: string; port: number } => {
  if (!string) {
    // fallback default values
    return { host: '127.0.0.1', port: 7890 }
  } else {
    const [host, port] = string.split(':')

    return { host, port: Number(port) }
  }
}

/**
 * @example
 * openWebUrl("http://localhost?server=%host", labels)
 *
 * @returns { void }
 * void
 *
 * `open clash external web url with browser.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const openWebUrl = (
  string: string,
  labels: {
    [label: string]: string | number | undefined | null
  },
): void => {
  let url = ''

  for (const key in labels) {
    const regex = new RegExp(`%${key}`, 'g')

    url = string.replace(regex, labels[key] as string)
  }

  openThat(url)
}

/**
 * @example
 * <Item>
 *  <Child />
 * </Item>
 *
 * `Material You list Item. Extend MuiPaper.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const Item = styled(Paper)<PaperProps>(({ theme }) => ({
  backgroundColor: alpha(theme.palette.primary.main, 0.1),
  padding: 16,
  borderRadius: 16,
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
})) as typeof Paper

export interface ClashWebItemProps {
  label: ReactNode
  onOpen: () => void
  onDelete: () => void
  onEdit: () => void
}

/**
 * @example
 * <ClashWebItem
    label={renderChip(item, labels)}
    onOpen={() => openWebUrl(item, labels)}
    onEdit={() => {
      setEditString(item);
      setOpen(true);
    }}
    onDelete={() => {}}
  />
  
 * `Clash Web UI list Item.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const ClashWebItem = ({
  label,
  onOpen,
  onDelete,
  onEdit,
}: ClashWebItemProps) => {
  return (
    <Item>
      <Marquee>
        <Typography variant="subtitle1" sx={{ marginRight: 16 }}>
          {label}
        </Typography>
      </Marquee>

      <Box display="flex" justifyContent="end" alignItems="center" gap={1}>
        <IconButton onClick={onOpen}>
          <OpenInNewRounded />
        </IconButton>

        <IconButton onClick={onEdit}>
          <EditRounded />
        </IconButton>

        <IconButton onClick={onDelete}>
          <DeleteRounded />
        </IconButton>
      </Box>
    </Item>
  )
}

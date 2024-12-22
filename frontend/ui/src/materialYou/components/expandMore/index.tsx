import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import IconButton, { IconButtonProps } from '@mui/material/IconButton'
import useTheme from '@mui/material/styles/useTheme'

interface ExpandMoreProps extends IconButtonProps {
  expand: boolean
  reverse?: boolean
}

/**
 * @example
 * <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
 *
 * `Built-in a small arrow icon.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const ExpandMore = ({ expand, reverse, ...props }: ExpandMoreProps) => {
  const { transitions } = useTheme()

  return (
    <IconButton {...props}>
      <ExpandMoreIcon
        sx={{
          transform: !expand
            ? reverse
              ? 'rotate(180deg)'
              : 'rotate(0deg)'
            : reverse
              ? 'rotate(0deg)'
              : 'rotate(180deg)',
          marginLeft: 'auto',
          transition: transitions.create('transform', {
            duration: transitions.duration.shortest,
          }),
        }}
      />
    </IconButton>
  )
}

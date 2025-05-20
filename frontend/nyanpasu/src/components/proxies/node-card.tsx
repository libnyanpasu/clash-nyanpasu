import { useLockFn } from 'ahooks'
import { CSSProperties, memo, useMemo } from 'react'
import Box from '@mui/material/Box'
import { ClashProxiesQueryProxyItem } from '@nyanpasu/interface'
import { alpha, cn } from '@nyanpasu/ui'
import { PaperSwitchButton } from '../setting/modules/system-proxy'
import DelayChip from './delay-chip'
import FeatureChip from './feature-chip'
import styles from './node-card.module.scss'
import { filterDelay } from './utils'

export const NodeCard = memo(function NodeCard({
  node,
  now,
  disabled,
  style,
}: {
  node: ClashProxiesQueryProxyItem
  now?: string | null
  disabled?: boolean
  style?: CSSProperties
}) {
  const delay = useMemo(() => filterDelay(node.history), [node.history])

  const checked = node.name === now

  const handleDelayClick = useLockFn(async () => {
    await node.mutateDelay()
  })

  const handleClick = useLockFn(async () => {
    await node.mutateSelect()
  })

  return (
    <PaperSwitchButton
      label={node.name}
      checked={checked}
      disableLoading
      onClick={handleClick}
      disabled={disabled}
      style={style}
      className={cn(styles.Card, delay === -1 && styles.NoDelay)}
      sxPaper={(theme) => ({
        backgroundColor: checked
          ? alpha(theme.vars.palette.primary.main, 0.3)
          : theme.vars.palette.grey[100],
        ...theme.applyStyles('dark', {
          backgroundColor: checked
            ? alpha(theme.vars.palette.primary.main, 0.3)
            : theme.vars.palette.grey[900],
        }),
      })}
    >
      <Box width="100%" display="flex" gap={0.5}>
        <FeatureChip label={node.type} />

        {node.udp && <FeatureChip label="UDP" />}

        <DelayChip
          className={styles.DelayChip}
          delay={delay}
          onClick={handleDelayClick}
        />
      </Box>
    </PaperSwitchButton>
  )
})

export default NodeCard

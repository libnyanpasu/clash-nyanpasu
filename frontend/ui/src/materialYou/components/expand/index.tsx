import { motion } from 'framer-motion'
import { ReactNode } from 'react'

/**
 * @example
 * <Expand open={true}></Expand>
 *
 * @returns {React.JSX.Element}
 * React.JSX.Element
 *
 * `With motion support.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const Expand = ({
  open,
  children,
}: {
  open: boolean
  children: ReactNode
}): React.JSX.Element => {
  return (
    <motion.div
      initial={false}
      animate={open ? 'open' : 'closed'}
      variants={{
        open: { opacity: 1, height: 'auto' },
        closed: { opacity: 0, height: 0 },
      }}
    >
      {children}
    </motion.div>
  )
}

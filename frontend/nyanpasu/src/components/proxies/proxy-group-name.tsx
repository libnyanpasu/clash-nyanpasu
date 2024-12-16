import { AnimatePresence, motion } from 'framer-motion'
import { memo } from 'react'
import { useNyanpasu } from '@nyanpasu/interface'

export const ProxyGroupName = memo(function ProxyGroupName({
  name,
}: {
  name: string
}) {
  const { nyanpasuConfig } = useNyanpasu()

  const disbaleMotion = nyanpasuConfig?.lighten_animation_effects

  return disbaleMotion ? (
    <>{name}</>
  ) : (
    <AnimatePresence mode="sync" initial={false}>
      <motion.div
        key={`group-name-${name}`}
        className="absolute"
        initial={{ x: 100, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: -100, opacity: 0 }}
        transition={{
          type: 'spring',
          bounce: 0,
          duration: 0.5,
        }}
      >
        {name}
      </motion.div>
    </AnimatePresence>
  )
})

export default ProxyGroupName

import { AnimatePresence, motion } from 'framer-motion'
import { ReactNode } from 'react'
import { cn } from '@/utils'
import {
  Box,
  Card,
  CardContent,
  CircularProgress,
  Typography,
} from '@mui/material'
import style from './style.module.scss'

export const BaseCard = ({
  label,
  labelChildren,
  loading,
  children,
}: {
  label?: string
  labelChildren?: ReactNode
  loading?: boolean
  children?: ReactNode
}) => {
  return (
    <Card style={{ position: 'relative' }}>
      <CardContent>
        {label && (
          <Box
            display="flex"
            justifyContent="space-between"
            alignItems="center"
            sx={{ pb: 1 }}
          >
            <Typography variant="h5" component="div">
              {label}
            </Typography>

            {labelChildren}
          </Box>
        )}

        {children}
      </CardContent>

      <AnimatePresence initial={false}>
        {loading && (
          <motion.div
            className={cn(style.LoadingMask, 'bg-zinc-100/10')}
            initial={{
              opacity: 0,
            }}
            animate={{
              opacity: 1,
            }}
            exit={{
              opacity: 0,
            }}
          >
            <CircularProgress />
          </motion.div>
        )}
      </AnimatePresence>
    </Card>
  )
}

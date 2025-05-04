import { motion } from 'framer-motion'
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

      <motion.div
        initial={false}
        animate={loading ? 'loading' : 'none'}
        variants={{
          loading: { opacity: 1, visibility: 'visible' },
          none: {
            opacity: 0,
            transitionEnd: {
              visibility: 'hidden',
            },
          },
        }}
      >
        <Box className={cn(style.LoadingMask, 'bg-gray-100/10')}>
          <CircularProgress />
        </Box>
      </motion.div>
    </Card>
  )
}

import { CSSProperties, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { RadixBaseDialog } from '../../../components/radix'

export interface BaseDialogProps {
  title: ReactNode
  open: boolean
  close?: string
  ok?: string
  disabledOk?: boolean
  contentStyle?: CSSProperties
  children?: ReactNode
  loading?: boolean
  full?: boolean
  onOk?: () => void | Promise<void>
  onClose?: () => void
  divider?: boolean
}

export const BaseDialog = ({
  title,
  open,
  close,
  onClose,
  children,
  contentStyle,
  disabledOk,
  loading,
  full,
  onOk,
  ok,
  divider,
}: BaseDialogProps) => {
  const { t } = useTranslation()

  // Adapter: delegate rendering to the new Radix-based dialog while preserving API
  return (
    <RadixBaseDialog
      title={title}
      open={open}
      close={close ?? t('Close')}
      ok={ok ?? t('Ok')}
      disabledOk={disabledOk}
      contentStyle={contentStyle}
      loading={loading}
      full={full}
      onOk={onOk}
      onClose={onClose}
      divider={divider}
    >
      {children}
    </RadixBaseDialog>
  )
}

import { ChangeEvent, useState } from 'react'
import { SwitchProps } from '@mui/material'
import LoadingSwitch from '../loadingSwitch'
import { BaseItem } from './baseItem'

interface Props extends SwitchProps {
  label: string
  onChange?: (
    event: ChangeEvent<HTMLInputElement>,
    checked: boolean,
  ) => Promise<void> | void
}

export const SwitchItem = ({ label, onChange, ...switchProps }: Props) => {
  const [loading, setLoading] = useState(false)

  const handleChange = async (
    event: ChangeEvent<HTMLInputElement>,
    checked: boolean,
  ) => {
    if (onChange) {
      try {
        setLoading(true)

        await onChange(event, checked)
      } finally {
        setLoading(false)
      }
    }
  }

  return (
    <BaseItem title={label}>
      <LoadingSwitch
        loading={loading}
        onChange={handleChange}
        {...switchProps}
      />
    </BaseItem>
  )
}

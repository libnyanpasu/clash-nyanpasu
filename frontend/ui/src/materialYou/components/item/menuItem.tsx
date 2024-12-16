import { MenuItem as MuiMenuItem, Select, SxProps } from '@mui/material'
import { BaseItem } from './baseItem'

type OptionValue = string | number | boolean

export interface MenuItemProps {
  label: string
  options: Record<string, OptionValue>
  selected: OptionValue
  onSelected: (value: OptionValue) => void
  selectSx?: SxProps
  disabled?: boolean
}

/**
 * @example
 * <MenuItem
    label={t("Log Level")}
    options={options}
    selected={selected}
    onSelected={(value) => {
      console.log(value);
    }}
    selectSx={{ width: 100 }}
  />
 *
 * @returns {React.JSX.Element}
 * React.JSX.Element
 *
 * `MenuItem extends MuiMenuItem. Support options api.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const MenuItem = ({
  label,
  options,
  selected,
  onSelected,
  selectSx,
  disabled,
}: MenuItemProps) => {
  return (
    <BaseItem title={label}>
      <Select
        size="small"
        value={selected}
        inputProps={{ 'aria-label': 'Without label' }}
        onChange={(e) => {
          onSelected(e.target.value)
        }}
        sx={{ width: 104, ...selectSx }}
        disabled={disabled}
      >
        {Object.entries(options).map(([key, value]) => (
          <MuiMenuItem
            key={key}
            value={key}
            disabled={key === selected}
            selected={key === selected}
          >
            {value}
          </MuiMenuItem>
        ))}
      </Select>
    </BaseItem>
  )
}

export default MenuItem

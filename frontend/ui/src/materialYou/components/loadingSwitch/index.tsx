import CircularProgress from '@mui/material/CircularProgress'
import Switch, { SwitchProps } from '@mui/material/Switch'
import style from './style.module.scss'

interface LoadingSwitchProps extends SwitchProps {
  loading?: boolean
}

/**
 * @example
 * <LoadingSwitch
    loading={loading} 
    onChange={handleChange} 
    {...switchProps} 
  />
*
 * `Support loading status.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const LoadingSwitch = ({
  loading,
  checked,
  disabled,
  ...props
}: LoadingSwitchProps) => {
  return (
    <div className={style['MDYSwitch-container']}>
      {loading && (
        <CircularProgress
          className={
            checked ? style['CircularProgress-checked'] : style.CircularProgress
          }
          aria-labelledby={props.id}
          color="inherit"
          size={16}
        />
      )}
      <Switch disabled={loading || disabled} checked={checked} {...props} />
    </div>
  )
}

export default LoadingSwitch

import { useSetting } from '@nyanpasu/query'
import useCoreIcon from './use-core-icon'

export default function useCurrentCoreIcon() {
  const { value: currentCore } = useSetting('core')

  return useCoreIcon(currentCore)
}

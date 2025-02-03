import { useAtomValue } from 'jotai'
import { useWindowSize } from 'react-use'
import { useIsAppImage } from '@/hooks/use-consts'
import { atomIsDrawerOnlyIcon } from '@/store'
import Masonry from '@mui/lab/Masonry'
import SettingClashBase from './setting-clash-base'
import SettingClashCore from './setting-clash-core'
import SettingClashExternal from './setting-clash-external'
import SettingClashField from './setting-clash-field'
import SettingClashPort from './setting-clash-port'
import SettingClashWeb from './setting-clash-web'
import SettingNyanpasuMisc from './setting-nyanpasu-misc'
import SettingNyanpasuPath from './setting-nyanpasu-path'
import SettingNyanpasuTasks from './setting-nyanpasu-tasks'
import SettingNyanpasuUI from './setting-nyanpasu-ui'
import SettingNyanpasuVersion from './setting-nyanpasu-version'
import SettingNyanpasuWidget from './setting-nyanpasu-widget'
import SettingSystemBehavior from './setting-system-behavior'
import SettingSystemProxy from './setting-system-proxy'
import SettingSystemService from './setting-system-service'

export const SettingPage = () => {
  const isAppImage = useIsAppImage()

  const isDrawerOnlyIcon = useAtomValue(atomIsDrawerOnlyIcon)

  const { width } = useWindowSize()

  return (
    <Masonry
      className="w-full"
      columns={{
        xs: 1,
        sm: 1,
        md: isDrawerOnlyIcon ? 2 : width > 1000 ? 2 : 1,
        lg: 2,
        xl: 2,
      }}
      spacing={3}
      sequential
    >
      <SettingSystemProxy />

      <SettingNyanpasuUI />

      <SettingNyanpasuWidget />

      <SettingClashBase />

      <SettingClashPort />

      <SettingClashExternal />

      <SettingClashWeb />

      <SettingClashField />

      <SettingClashCore />

      <SettingSystemBehavior />

      {!isAppImage.data && <SettingSystemService />}

      <SettingNyanpasuTasks />

      <SettingNyanpasuMisc />

      <SettingNyanpasuPath />

      <SettingNyanpasuVersion />
    </Masonry>
  )
}

export default SettingPage

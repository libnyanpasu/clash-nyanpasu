import { WidgetImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { mutations, queries } from '@/services/rpc'
import { useWidgetIcon } from '@nyanpasu/query'
import { IconSetStoreProvider, type IconSetStore } from './icon-set-store'
import TrayIconConfig from './tray-icon-config'

// The widget's own icons. Unlike the tray, the widget draws coloured artwork
// on every platform, so the presets are offered everywhere.
const WIDGET_ICON_STORE: IconSetStore = {
  useIcon: (mode) => useWidgetIcon(mode),
  Image: WidgetImage,
  isSetQuery: (mode) => queries.isWidgetIconSet(mode),
  setIcon: mutations.setWidgetIcon,
  setIconFromBytes: mutations.setWidgetIconFromBytes,
  iconKey: 'getWidgetIcon',
  isSetKey: 'isWidgetIconSet',
  presets: true,
  messages: {
    label: m.settings_nyanpasu_network_statistic_widget_logo_label,
    presetTitle: m.settings_nyanpasu_network_statistic_widget_logo_preset_title,
    setSuccess: m.settings_nyanpasu_network_statistic_widget_logo_set_success,
    setFailed: m.settings_nyanpasu_network_statistic_widget_logo_set_failed,
  },
}

export default function NetworkStatisticWidgetIconConfig() {
  return (
    <IconSetStoreProvider value={WIDGET_ICON_STORE}>
      <TrayIconConfig />
    </IconSetStoreProvider>
  )
}

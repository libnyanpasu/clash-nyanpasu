import {
  Apps,
  Dashboard,
  DesignServices,
  GridView,
  Public,
  Settings,
  SettingsEthernet,
  SvgIconComponent,
  Terminal,
} from '@mui/icons-material'

const routes: { [key: string]: SvgIconComponent } = {
  dashboard: Dashboard,
  proxies: Public,
  profiles: GridView,
  connections: SettingsEthernet,
  rules: DesignServices,
  logs: Terminal,
  settings: Settings,
  providers: Apps,
}

export const getRoutes = () => {
  return Object.keys(routes).reduce(
    (acc, key) => {
      acc[key] = `/${key}`
      return acc
    },
    {} as { [key: string]: string },
  )
}

export const getRoutesWithIcon = () => {
  return Object.keys(routes).reduce(
    (acc, key) => {
      acc[key] = {
        path: `/${key}`,
        icon: routes[key],
      }
      return acc
    },
    {} as {
      [key: string]: { path: string; icon: SvgIconComponent }
    },
  )
}

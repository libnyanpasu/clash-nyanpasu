const EXPERIMENTAL_ROUTER_KEY = 'enabled-experimental-router'

export const getEnabledExperimentalRouter = () => {
  return localStorage.getItem(EXPERIMENTAL_ROUTER_KEY) === 'true'
}

export const setEnabledExperimentalRouter = (enabled: boolean) => {
  localStorage.setItem(EXPERIMENTAL_ROUTER_KEY, enabled.toString())
}

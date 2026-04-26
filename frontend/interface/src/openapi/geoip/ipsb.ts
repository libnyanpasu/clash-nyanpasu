export interface IPSBResponse {
  organization: string
  longitude: number
  timezone: string
  isp: string
  offset: number
  asn: number
  asn_organization: string
  country: string
  ip: string
  latitude: number
  continent_code: string
  country_code: string
}

// export const useIPSB = (config?: SWRConfiguration) => {
//   return useSWR(
//     'https://api.ip.sb/geoip',
//     async () =>
//       unwrapResult(await commands.getIpsbAsn()) as unknown as IPSBResponse,
//     config,
//   )
// }

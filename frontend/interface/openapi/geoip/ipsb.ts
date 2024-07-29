import { ofetch } from "ofetch";
import useSWR from "swr";

interface IPSBResponse {
  organization: string;
  longitude: number;
  timezone: string;
  isp: string;
  offset: number;
  asn: number;
  asn_organization: string;
  country: string;
  ip: string;
  latitude: number;
  continent_code: string;
  country_code: string;
}

export const useIPSB = () => {
  return useSWR(
    "https://api.ip.sb/geoip",
    async () => await ofetch<IPSBResponse>("https://api.ip.sb/geoip"),
  );
};

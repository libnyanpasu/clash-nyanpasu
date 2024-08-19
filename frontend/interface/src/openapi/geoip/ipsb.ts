import useSWR from "swr";
import { getIpsbASN } from "@/service";

export interface IPSBResponse {
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
  return useSWR("https://api.ip.sb/geoip", () => getIpsbASN());
};

import { Clash, clash as clashApi } from "@/service";
import * as tauri from "@/service/tauri";
import useSWR from "swr";

export const useClashCore = () => {
  const { getGroupDelay, getProxiesDelay, ...clash } = clashApi();

  const { data, isLoading, mutate } = useSWR("getProxies", tauri.getProxies);

  const updateGroupDelay = async (
    index: number,
    options?: Clash.DelayOptions,
  ) => {
    const group = data?.groups[index];

    if (!group) {
      return;
    }

    await getGroupDelay(group?.name, options);

    await mutate();
  };

  const updateProxiesDelay = async (
    name: string,
    options?: Clash.DelayOptions,
  ) => {
    const result = await getProxiesDelay(name, options);

    await mutate();

    return result;
  };

  const setGroupProxy = async (index: number, name: string) => {
    const group = data?.groups[index];

    if (!group) {
      return;
    }

    await tauri.selectProxy(group?.name, name);

    await mutate();
  };

  const getRules = useSWR("getRules", clash.getRules);

  return {
    data,
    isLoading,
    updateGroupDelay,
    updateProxiesDelay,
    setGroupProxy,
    getRules,
  };
};

// export class UseClashCore {
//   public proxies;

//   constructor() {
//     this.proxies = useSWR("getProxies", getProxies);
//   }

//   public async updateGroupDelay(index: number, options?: Clash.DelayOptions) {
//     console.log(index);
//     // const group = this.proxies.data?.groups[index];
//     console.log(this.proxies.data?.groups);

//     // if (!group) {
//     //   return;
//     // }

//     // const result = await getGroupDelay(group?.name, options);

//     // console.log(result);

//     // group.all?.forEach((item) => {
//     //   if (result)
//     // })

//     // Object.entries(result).forEach(([name, delay]) => {

//     // })
//   }
// }

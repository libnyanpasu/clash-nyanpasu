import { Clash, clash } from "@/service";
import * as tauri from "@/service/tauri";
import useSWR from "swr";

export const useClashCore = () => {
  const { getGroupDelay } = clash();

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

  const setGroupProxy = async (index: number, name: string) => {
    const group = data?.groups[index];

    if (!group) {
      return;
    }

    await tauri.selectProxy(group?.name, name);

    await mutate();
  };

  return {
    data,
    isLoading,
    updateGroupDelay,
    setGroupProxy,
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

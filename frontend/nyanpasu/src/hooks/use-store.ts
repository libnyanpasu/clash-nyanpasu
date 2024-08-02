import { useAtom } from "jotai";
import { coreTypeAtom } from "@/store/clash";
import { useNyanpasu } from "@nyanpasu/interface";

export function useCoreType() {
  const [coreType, setCoreType] = useAtom(coreTypeAtom);
  const { setNyanpasuConfig } = useNyanpasu({
    onSuccess(data) {
      setCoreType(data?.clash_core || "mihomo");
    },
  });
  const setter = (value: typeof coreType) => {
    setCoreType(value);
    setNyanpasuConfig({ clash_core: value });
  };
  return [coreType, setter] as const;
}

import { useAtom } from "jotai";
import { useEffect } from "react";
import { coreTypeAtom } from "@/store/clash";
import { useNyanpasu } from "@nyanpasu/interface";

export function useCoreType() {
  const [coreType, setCoreType] = useAtom(coreTypeAtom);
  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu({
    onUpdate(data) {
      setCoreType(data?.clash_core || "mihomo");
    },
  });
  useEffect(() => {
    if (nyanpasuConfig?.clash_core !== coreType) {
      setNyanpasuConfig({ clash_core: coreType });
    }
  }, [coreType]);
  return [coreType, setCoreType] as const;
}

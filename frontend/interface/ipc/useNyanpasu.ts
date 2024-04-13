import useSWR from "swr";
import { nyanpasuConfig, VergeConfig } from "@/service";

export const useNyanpasu = (options?: {
  onUpdate?: (data?: VergeConfig) => void;
  onError?: (error: any) => void;
}) => {
  const { data, error, mutate } = useSWR<VergeConfig>("nynpasuConfig", () =>
    nyanpasuConfig.get(),
  );

  const setNyanpasuConfig = async (payload: Partial<VergeConfig>) => {
    try {
      await nyanpasuConfig.set(payload);

      const result = await mutate();

      if (options?.onUpdate) {
        options?.onUpdate(result);
      }
    } catch (error) {
      if (options?.onError) {
        options?.onError(error);
      }
    }
  };

  return {
    nyanpasuConfig: data,
    isLoading: !data && !error,
    isError: error,
    setNyanpasuConfig,
  };
};

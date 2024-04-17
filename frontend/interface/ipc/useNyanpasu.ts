import useSWR from "swr";
import { getNyanpasuConfig, patchNyanpasuConfig, VergeConfig } from "@/service";

/**
 * useNyanpasu with swr.
 * Data from tauri backend.
 */
export const useNyanpasu = (options?: {
  onUpdate?: (data?: VergeConfig) => void;
  onError?: (error: any) => void;
}) => {
  const { data, error, mutate } = useSWR<VergeConfig>(
    "nyanpasuConfig",
    getNyanpasuConfig,
  );

  const setNyanpasuConfig = async (payload: Partial<VergeConfig>) => {
    try {
      await patchNyanpasuConfig(payload);

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

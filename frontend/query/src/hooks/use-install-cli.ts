import { useMutation } from '@tanstack/react-query'
import { invokeMutation } from '../ipc/query-options'
import { useQueryApi } from '../provider/rpc-provider'

/**
 * Links the bundled `nyanpasu-cli` onto the user's `PATH`. The result says where it was linked,
 * whether that folder is on `PATH`, and the command that installs it for every user instead.
 */
export function useInstallCli() {
  const api = useQueryApi()

  const command = api.mutations.installCli()

  return useMutation({
    mutationKey: command.mutationKey,
    mutationFn: async () => await invokeMutation(command),
  })
}

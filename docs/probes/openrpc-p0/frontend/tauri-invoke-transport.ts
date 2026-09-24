import { getBatchRequests } from "@open-rpc/client-js/build/Request.js";
import type { JSONRPCRequestData } from "@open-rpc/client-js/build/Request.js";
import { ERR_UNKNOWN, JSONRPCError } from "@open-rpc/client-js/build/Error.js";
import { Transport } from "@open-rpc/client-js/build/transports/Transport.js";

export type InvokeRpc = (request: string) => Promise<string>;

export class TauriInvokeTransport extends Transport {
  private readonly invokeRpc: InvokeRpc;

  constructor(invokeRpc: InvokeRpc) {
    super();
    this.invokeRpc = invokeRpc;
  }

  async connect(): Promise<void> {}

  close(): void {}

  async sendData(data: JSONRPCRequestData, timeout: number | null = null): Promise<unknown> {
    const pending = this.transportRequestManager.addRequest(data, timeout);
    const requests = data instanceof Array ? getBatchRequests(data) : [data];

    try {
      const response = await this.invokeRpc(JSON.stringify(this.parseData(data)));
      const responseError = this.transportRequestManager.resolveResponse(response);
      if (responseError) {
        this.transportRequestManager.settlePendingRequest(requests, responseError);
        return Promise.reject(responseError);
      }
      return pending;
    } catch (cause) {
      const error = cause instanceof JSONRPCError
        ? cause
        : new JSONRPCError(cause instanceof Error ? cause.message : String(cause), ERR_UNKNOWN, cause);
      this.transportRequestManager.settlePendingRequest(requests, error);
      return Promise.reject(error);
    }
  }
}

export type ApiRequest = {
  method: string;
  uri: string;
  headers: Record<string, string>;
  body?: string;
};

export type ApiResponse = {
  status: number;
  headers: Record<string, string>;
  body: string;
};

export type ApiDispatch = (request: ApiRequest) => Promise<ApiResponse>;
export type ApiFetch = <T>(url: string, init?: RequestInit) => Promise<T>;

export function createApiFetch(dispatch: ApiDispatch): ApiFetch {
  return async function apiFetch<T>(url: string, init?: RequestInit): Promise<T> {
    if (init?.signal?.aborted) {
      throw new DOMException("The request was aborted before dispatch", "AbortError");
    }

    if (init?.body !== undefined && init.body !== null && typeof init.body !== "string") {
      throw new TypeError("The RPC bridge probe only accepts string request bodies");
    }

    const parsedUrl = new URL(url, "http://api.invalid");
    const response = await dispatch({
      method: init?.method ?? "GET",
      uri: `${parsedUrl.pathname}${parsedUrl.search}`,
      headers: Object.fromEntries(new Headers(init?.headers).entries()),
      body: typeof init?.body === "string" ? init.body : undefined,
    });

    const data = response.body ? JSON.parse(response.body) : undefined;
    if (response.status >= 400) {
      throw data;
    }

    // Orval's fetch client expects its standard { data, status, headers } result.
    return {
      data,
      status: response.status,
      headers: new Headers(response.headers),
    } as T;
  };
}

let configuredApiFetch: ApiFetch | undefined;

export function configureApiDispatch(dispatch: ApiDispatch) {
  configuredApiFetch = createApiFetch(dispatch);
}

export async function apiFetch<T>(url: string, init?: RequestInit): Promise<T> {
  if (!configuredApiFetch) {
    throw new Error("Configure the host API transport before using generated clients");
  }

  return configuredApiFetch<T>(url, init);
}

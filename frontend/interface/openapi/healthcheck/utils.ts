import { ofetch } from "ofetch";

export const $request = ofetch.create({
  mode: "no-cors",
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const timingFn = async (func: () => Promise<any>) => {
  const start = performance.now();

  await func();

  const end = performance.now();

  return end - start;
};

export const timing = async (url: string) => {
  return await timingFn(() => $request(url));
};

export const createTiming = (url: string) => {
  return () => timing(url);
};

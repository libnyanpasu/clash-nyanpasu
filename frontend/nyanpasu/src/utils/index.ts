/**
 * classNames filter out falsy values and join the rest with a space
 * @param classes - array of classes
 * @returns string of classes
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function classNames(...classes: any[]) {
  return classes.filter(Boolean).join(" ");
}

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

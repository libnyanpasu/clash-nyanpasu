import { includes, isArray, isObject, isString, some } from "lodash-es";

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

export const containsSearchTerm = (obj: any, term: string): boolean => {
  if (!obj || !term) return false;

  if (isString(obj)) {
    return includes(obj.toLowerCase(), term.toLowerCase());
  }

  if (isObject(obj) || isArray(obj)) {
    return some(obj, (value: any) => containsSearchTerm(value, term));
  }

  return false;
};

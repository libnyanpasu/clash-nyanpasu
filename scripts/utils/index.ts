import { GITHUB_PROXY } from "./env";

export const getGithubUrl = (url: string) => {
  return new URL(url.replace(/^https?:\/\//g, ""), GITHUB_PROXY).toString();
};

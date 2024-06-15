import { Octokit } from "octokit";

const BASE_OPTIONS = {
  owner: "LibNyanpasu",
  repo: "clash-nyanpasu",
};

export const octokit = new Octokit(BASE_OPTIONS);

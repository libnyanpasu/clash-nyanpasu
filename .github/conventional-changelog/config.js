"use strict";
const conventionalChangelogConfig = require("conventional-changelog-conventionalcommits");
const github = require("@actions/github");
const fs = require("node:fs");
const dedent = require("dedent");

const GIT_COMMIT_WITH_AUTHOR_FORMAT =
  "%B%n-hash-%n%H%n-gitTags-%n%d%n-committerDate-%n%ci%n-authorName-%n%an%n-authorEmail-%n%ae%n-gpgStatus-%n%G?%n-gpgSigner-%n%GS";

const extraCommitMsg = `by @{{userLogin}}`;

const QUERY_PAGE_SIZE = 100;

function writerOptsTransform(
  originalTransform,
  commitsSinceLastRelease,
  commit,
  context,
) {
  // execute original writerOpts transform
  const extendedCommit = originalTransform(commit, context);

  // then add client remote detail (login) to the commit object and return it
  if (extendedCommit) {
    // search current commit with the commits since last release array returned from fetching GitHub API
    const remoteCommit = commitsSinceLastRelease.find(
      (c) => c.shortHash === commit.shortHash,
    );
    if (remoteCommit?.login) {
      commit.userLogin = remoteCommit.login;
    }
  }

  return extendedCommit;
}

/**
 * From a dot (.) notation path, find and return a property within an object given a complex object path
 * Note that the object path does should not include the parent itself
 * for example if we want to get `address.zip` from `user` object, we would call `getComplexObjectValue(user, 'address.zip')`
 * @param object - object to search from
 * @param path - complex object path to find descendant property from, must be a string with dot (.) notation
 * @returns outputValue - the object property value found if any
 */
function getComplexObjectValue(object, path) {
  if (!object || !path) {
    return object;
  }
  return path.split(".").reduce((obj, prop) => obj?.[prop], object);
}

// Retrieve previous commits since last release from GitHub API
async function retrievePreviousCommits(branchName) {
  const octokit = github.getOctokit(process.env.GITHUB_TOKEN);

  // first retrieve the latest tag
  const {
    data: { tag_name },
  } = await octokit.rest.repos.getLatestRelease({
    owner: "keiko233",
    repo: "clash-nyanpasu",
  });

  // then retrieve the latest tag commit timestamp
  const { data: commitData } = await octokit.rest.repos.getCommit({
    owner: "keiko233",
    repo: "clash-nyanpasu",
    ref: tag_name,
  });

  const sinceDate =
    commitData.commit.committer.date || commitData.commit.author.date;

  const remoteCommits = [];
  let afterCursor = "";
  let hasNextPage = false;

  do {
    const afterCursorStr = afterCursor ? `, after: "${afterCursor}"` : "";
    const queryStr = dedent`
        query getCommits($repo: String!, $owner: String!, $branchName: String!, $pageSize: Int!, $since: GitTimestamp!) {
            repository(name: $repo, owner: $owner) {
              ref(qualifiedName: $branchName) {
                target { ... on Commit {
                    history(first: $pageSize, since: $since ${afterCursorStr}) {
                      nodes { oid, message, author { name, user { login }}}
                      pageInfo { hasNextPage, endCursor }
          }}}}}}
          `.trim();

    const response = await octokit.graphql(queryStr, {
      owner: "keiko233",
      repo: "clash-nyanpasu",
      afterCursor,
      branchName,
      pageSize: QUERY_PAGE_SIZE,
      since: sinceDate,
    });

    const historyData = getComplexObjectValue(
      response,
      "repository.ref.target.history",
    );
    const pageInfo = historyData?.pageInfo;
    hasNextPage = pageInfo?.hasNextPage ?? false;
    afterCursor = pageInfo?.endCursor ?? "";

    if (historyData?.nodes) {
      for (const commit of historyData.nodes) {
        if (commit?.oid && commit?.author) {
          remoteCommits.push({
            shortHash: commit.oid.substring(0, 7),
            authorName: commit?.author.name,
            login: commit?.author?.user?.login ?? "",
            message: commit?.message ?? "",
          });
        }
      }
    }
  } while (hasNextPage);

  console.log(
    "github",
    "found %s commits since last release timestamp %s",
    remoteCommits.length,
    sinceDate,
  );

  return remoteCommits;
}

module.exports = (async () => {
  const config = await conventionalChangelogConfig({
    types: [
      {
        type: "feat",
        section: "✨ Features",
      },
      {
        type: "fix",
        section: "🐛 Bug Fixes",
      },
      {
        type: "chore",
        section: "🧹 Maintenance",
      },
      {
        type: "docs",
        section: "📚 Documentation",
      },
      {
        type: "style",
        section: "💅 Styles",
      },
      {
        type: "refactor",
        section: "🔨 Refactoring",
      },
      {
        type: "perf",
        section: "⚡ Performance Improvements",
      },
      {
        type: "test",
        section: "✅ Tests",
      },
    ],
  });
  const commitsSinceLastRelease = await retrievePreviousCommits("main");
  config.gitRawCommitsOpts.format = GIT_COMMIT_WITH_AUTHOR_FORMAT;
  config.writerOpts.commitPartial =
    config.writerOpts.commitPartial.replace(/\n*$/, "") +
    ` {{#if @root.linkReferences~}}${extraCommitMsg}{{~/if}}\n`;
  config.writerOpts.transform = writerOptsTransform.bind(
    null,
    config.writerOpts.transform,
    commitsSinceLastRelease,
  );
  return config;
})();

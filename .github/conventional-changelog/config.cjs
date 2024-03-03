// modified from https://github.com/lerna-lite/lerna-lite/blob//v1.9.0/packages/core/src/conventional-commits/get-github-commits.ts
// ref: https://github.com/conventional-changelog/conventional-changelog/issues/349#issuecomment-1200070203
"use strict";
const conventionalChangelogConfig = require("conventional-changelog-conventionalcommits");
const github = require("@actions/github");
const { execSync, spawnSync } = require("node:child_process");
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

function trimChars(str, cutset) {
  let start = 0,
    end = str.length;

  while (start < end && cutset.indexOf(str[start]) >= 0) ++start;

  while (end > start && cutset.indexOf(str[end - 1]) >= 0) --end;

  return start > 0 || end < str.length ? str.substring(start, end) : str;
}

/**
 * Parse git output and return relevant metadata.
 * @param {string} stdout Result of `git describe`
 * @param {string} [cwd] Defaults to `process.cwd()`
 * @param [separator] Separator used within independent version tags, defaults to @
 * @returns {DescribeRefFallbackResult|DescribeRefDetailedResult}
 */
function parse(stdout, cwd, separator) {
  separator = separator || "@";
  const minimalShaRegex = /^([0-9a-f]{7,40})(-dirty)?$/;
  // when git describe fails to locate tags, it returns only the minimal sha
  if (minimalShaRegex.test(stdout)) {
    // repo might still be dirty
    const [, sha, isDirty] = minimalShaRegex.exec(stdout);

    // count number of commits since beginning of time
    const refCount = trimChars(
      spawnSync("git", ["rev-list", "--count", sha], { cwd }).stdout.toString(),
      "\n \r",
    );

    return { refCount, sha, isDirty: Boolean(isDirty) };
  }

  // If the user has specified a custom separator, it may not be regex-safe, so escape it
  const escapedSeparator = separator.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const regexPattern = new RegExp(
    `^((?:.*${escapedSeparator})?(.*))-(\\d+)-g([0-9a-f]+)(-dirty)?$`,
  );

  const [, lastTagName, lastVersion, refCount, sha, isDirty] =
    regexPattern.exec(stdout) || [];

  return { lastTagName, lastVersion, refCount, sha, isDirty: Boolean(isDirty) };
}

function getArgs(options, includeMergedTags) {
  let args = [
    "describe",
    // fallback to short sha if no tags located
    "--always",
    // always return full result, helps identify existing release
    "--long",
    // annotate if uncommitted changes present
    "--dirty",
    // prefer tags originating on upstream branch
    "--first-parent",
  ];

  if (options.match) {
    args.push("--match", options.match);
  }

  if (includeMergedTags) {
    // we want to consider all tags, also from merged branches
    args = args.filter((arg) => arg !== "--first-parent");
  }

  return args;
}

function describeRefSync(options = {}, includeMergedTags, dryRun = false) {
  console.error(
    "git",
    "describeRefSync",
    getArgs(options, includeMergedTags),
    // options,
    // dryRun,
  );
  const stdout = trimChars(
    spawnSync(
      "git",
      getArgs(options, includeMergedTags),
      // options,
      // dryRun,
    ).stdout.toString("utf8"),
    "\n \r",
  );
  const result = parse(stdout, options.cwd, options.separator);

  if (options?.match) {
    console.error("git-describe.sync", "%j => %j", options?.match, stdout);
  }
  if (stdout) {
    console.log(stdout);
    console.error("git-describe", "parsed => %j", result);
  }

  return result;
}

function getOldestCommitSinceLastTag(
  execOpts,
  isIndependent,
  includeMergedTags,
) {
  let commitResult = "";
  const describeOptions = { ...execOpts };
  if (isIndependent) {
    describeOptions.match = "*@*"; // independent tag pattern
  }
  const { lastTagName } = describeRefSync(describeOptions, includeMergedTags);

  if (lastTagName) {
    const gitCommandArgs = [
      "log",
      `${lastTagName}..HEAD`,
      '--format="%h %aI"',
      "--reverse",
    ];
    console.error("git", "getCurrentBranchOldestCommitSinceLastTag");
    console.error("exec", `git ${gitCommandArgs.join(" ")}`);
    let stdout = trimChars(
      spawnSync(
        "git",
        gitCommandArgs,
        // execOpts
      ).stdout.toString("utf8"),
      "\n \r",
    );
    if (!stdout) {
      // in some occasion the previous git command might return nothing, in that case we'll return the tag detail instead
      stdout = trimChars(
        spawnSync(
          "git",
          ["log", "-1", '--format="%h %aI"', lastTagName],
          // execOpts,
        ).stdout.toString() || "",
        "\n \r",
      );
    }
    [commitResult] = stdout.split("\n");
  } else {
    const gitCommandArgs = [
      "log",
      "--oneline",
      '--format="%h %aI"',
      "--reverse",
      "--max-parents=0",
      "HEAD",
    ];
    console.error("git", "getCurrentBranchFirstCommit");
    console.error("exec", `git ${gitCommandArgs.join(" ")}`);
    commitResult = trimChars(
      spawnSync(
        "git",
        gitCommandArgs,
        // execOpts
      ).stdout.toString("utf8"),
      "\n \r",
    );
  }

  const [, commitHash, commitDate] =
    /^"?([0-9a-f]+)\s([0-9\-|\+T\:]*)"?$/.exec(commitResult) || [];
  // prettier-ignore
  console.error('oldestCommitSinceLastTag', `commit found since last tag: ${lastTagName} - (SHA) ${commitHash} - ${commitDate}`);

  return { commitHash, commitDate };
}

async function getCommitsSinceLastRelease(branchName, isIndependent, execOpts) {
  // get the last release tag date or the first commit date if no release tag found
  const { commitDate } = getOldestCommitSinceLastTag(
    execOpts,
    isIndependent,
    false,
  );

  return getGithubCommits(branchName, commitDate, execOpts);
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

function getOldestCommitSinceLastTag(
  execOpts,
  isIndependent,
  includeMergedTags,
) {
  let commitResult = "";
  const describeOptions = { ...execOpts };
  if (isIndependent) {
    describeOptions.match = "*@*"; // independent tag pattern
  }
  const { lastTagName } = describeRefSync(describeOptions, includeMergedTags);

  if (lastTagName) {
    const gitCommandArgs = [
      "log",
      `${lastTagName}..HEAD`,
      '--format="%h %aI"',
      "--reverse",
    ];
    console.error("git", "getCurrentBranchOldestCommitSinceLastTag");
    console.error("exec", `git ${gitCommandArgs.join(" ")}`);
    let stdout = trimChars(
      spawnSync(
        "git",
        gitCommandArgs,
        // execOpts
      ).stdout.toString(),
      "\n \r",
    );
    if (!stdout) {
      // in some occasion the previous git command might return nothing, in that case we'll return the tag detail instead
      stdout = trimChars(
        spawnSync(
          "git",
          ["log", "-1", '--format="%h %aI"', lastTagName],
          // execOpts,
        ).stdout.toString() || "",
        "\n \r",
      );
    }
    [commitResult] = stdout.split("\n");
  } else {
    const gitCommandArgs = [
      "log",
      "--oneline",
      '--format="%h %aI"',
      "--reverse",
      "--max-parents=0",
      "HEAD",
    ];
    console.error("git", "getCurrentBranchFirstCommit");
    console.error("exec", `git ${gitCommandArgs.join(" ")}`);
    commitResult = trimChars(
      spawnSync(
        "git",
        gitCommandArgs,
        // execOpts
      ).stdout.toString(),
      "\n \r",
    );
  }

  const [, commitHash, commitDate] =
    /^"?([0-9a-f]+)\s([0-9\-|\+T\:]*)"?$/.exec(commitResult) || [];
  // prettier-ignore
  console.error('oldestCommitSinceLastTag', `commit found since last tag: ${lastTagName} - (SHA) ${commitHash} - ${commitDate}`);

  return { commitHash, commitDate };
}

// Retrieve previous commits since last release from GitHub API
async function getGithubCommits(branchName, sinceDate, execOpts) {
  const octokit = github.getOctokit(process.env.GITHUB_TOKEN);
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

  console.error(
    "github",
    "found %s commits since last release timestamp %s",
    remoteCommits.length,
    sinceDate,
  );

  return remoteCommits;
}

module.exports = (async () => {
  try {
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
    const commitsSinceLastRelease = await getCommitsSinceLastRelease(
      "main",
      false,
    );
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
  } catch (e) {
    console.error("pre-changelog-generation", e);
    process.exit(1);
  }
})();

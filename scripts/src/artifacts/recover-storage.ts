import { parseArgs } from "jsr:@std/cli@1/parse-args";
import * as path from "jsr:@std/path";
import { CENTRAL_PUBLICATION_TARGETS } from "./central-publication.ts";
import { runPublicationTasks } from "./publication-concurrency.ts";
import { runArchivePublish } from "./internet-archive-upload.ts";
import { sourceforgeDownloadUrl } from "./sourceforge.ts";
import { verifySourceforgeMirror } from "./verify-sourceforge-mirror.ts";

if (import.meta.main) {
  const args = parseArgs(Deno.args, {
    string: ["mode", "backend", "target", "publication-dir", "reports-dir"],
    boolean: ["defer-sourceforge-verification"],
  });
  if (
    !["register", "upload", "verify"].includes(args.mode ?? "") ||
    !["both", "archive", "sourceforge"].includes(args.backend ?? "") ||
    !args["publication-dir"] || !args["reports-dir"] ||
    (args.target !== "all" &&
      !CENTRAL_PUBLICATION_TARGETS.some((target) => target === args.target))
  ) {
    throw new Error(
      "Expected --mode register|upload|verify --backend both|archive|sourceforge --target all|<target> --publication-dir <dir> --reports-dir <dir>",
    );
  }
  if (args.mode === "register" && args.backend === "sourceforge") {
    throw new Error("Registration-only mode requires the archive backend");
  }
  if (
    args["defer-sourceforge-verification"] &&
    (args.mode !== "upload" || args.backend !== "sourceforge")
  ) {
    throw new Error(
      "Deferred verification requires a SourceForge-only upload followed by its independent verification job",
    );
  }
  const publicationDir = args["publication-dir"];
  const reportsDir = args["reports-dir"];
  const context = JSON.parse(
    await Deno.readTextFile(
      path.join(publicationDir, "publication-context.json"),
    ),
  );
  const targets = args.target === "all"
    ? CENTRAL_PUBLICATION_TARGETS
    : [args.target!];
  let failures = 0;
  let sourceforgeSuccesses = 0;
  let sourceforgeMirror:
    | Awaited<ReturnType<typeof verifySourceforgeMirror>>
    | undefined;
  const results = await runPublicationTasks(
    targets,
    2,
    async (target, index) => {
      console.log(
        `[storage] ${args.mode} ${args.backend} target ${target} (${
          index + 1
        }/${targets.length})`,
      );
      const manifestPath = path.resolve(
        publicationDir,
        "manifests",
        `${target}.json`,
      );
      const manifest = JSON.parse(await Deno.readTextFile(manifestPath));
      const reports = path.resolve(reportsDir, target);
      await Deno.mkdir(reports, { recursive: true });
      if (args.backend !== "archive" && args.mode !== "register") {
        try {
          if (args.mode === "upload") {
            const result = await new Deno.Command("deno", {
              args: [
                "task",
                "sourceforge:upload",
                manifestPath,
                path.join(reports, "sourceforge-report.json"),
              ],
              env: {
                SOURCEFORGE_CHANNEL: context.channel,
                SOURCEFORGE_BUILD_ID: context.buildId,
                SOURCEFORGE_RELEASE_TAG: context.tag ?? "",
              },
              stdout: "inherit",
              stderr: "inherit",
            }).output();
            if (!result.success) {
              throw new Error(`SourceForge upload failed for ${target}`);
            }
          }
          if (args["defer-sourceforge-verification"]) return;
          const project = Deno.env.get("SOURCEFORGE_PROJECT") ?? "";
          const remotePath = `${
            context.channel === "nightly" ? "nightly" : "releases"
          }/${context.buildId}`;
          const candidate = {
            schemaVersion: 1,
            status: "uploaded",
            project,
            target,
            channel: context.channel,
            buildId: context.buildId,
            remotePath,
            publishedAt: context.publishedAt,
            artifacts: manifest.artifacts.map((
              artifact: { fileName: string; fileSize: number; sha256: string },
            ) => ({
              fileName: artifact.fileName,
              fileSize: artifact.fileSize,
              sha256: artifact.sha256,
              url: sourceforgeDownloadUrl(
                project,
                remotePath,
                artifact.fileName,
              ),
            })),
          };
          const verified = await verifySourceforgeMirror(
            [candidate],
            [target],
            context.channel,
          );
          sourceforgeSuccesses++;
          sourceforgeMirror = {
            ...verified,
            assets: { ...sourceforgeMirror?.assets, ...verified.assets },
          };
          await Deno.writeTextFile(
            path.join(reports, "sourceforge-verified.json"),
            JSON.stringify(verified, null, 2),
          );
          // Only preserve success after every public byte has passed verification.
          await Deno.writeTextFile(
            path.join(reports, "sourceforge-report.json"),
            JSON.stringify(candidate, null, 2),
          );
        } catch (error) {
          console.error(error);
          await Deno.writeTextFile(
            path.join(reports, "sourceforge-verification-error.json"),
            JSON.stringify(
              {
                target,
                error: error instanceof Error ? error.message : String(error),
              },
              null,
              2,
            ),
          );
          failures++;
        }
      }
      if (args.backend !== "sourceforge") {
        const status = await runArchivePublish([
          ...(args.mode === "verify"
            ? ["--verify-only", "--build-id", manifest.buildId]
            : ["--manifest", manifestPath]),
          ...(args.mode === "register" ? ["--register-only"] : []),
          "--server",
          "https://archive.nyanpasu.org",
          "--report",
          path.join(reports, "ia-report.json"),
        ]);
        if (status !== 0 && status !== 2) failures++;
      }
    },
  );
  for (const result of results) {
    if (result.status === "rejected") {
      console.error(result.reason);
      failures++;
    }
  }
  if (sourceforgeMirror && sourceforgeSuccesses === targets.length) {
    await Deno.writeTextFile(
      path.join(reportsDir, "sourceforge-mirrors.json"),
      JSON.stringify(sourceforgeMirror, null, 2),
    );
  }
  if (failures) {
    throw new Error(
      `${failures} storage operation(s) failed; inspect the retained reports`,
    );
  }
}

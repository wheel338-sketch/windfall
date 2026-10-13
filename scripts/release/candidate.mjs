import fs from "node:fs";
import path from "node:path";
import {
  BLOCKERS,
  check,
  digest,
  fields,
  identity,
  installerNames,
  provenance,
  target,
} from "./contracts.mjs";
import { captureDependencies, validateDependencies } from "./dependencies.mjs";
import {
  compare,
  fileRecord,
  inside,
  json,
  load,
  portable,
  rootPath,
  sha256,
  stage,
  unique,
  walk,
  writeNew,
} from "./files.mjs";
import {
  checkCapture,
  resources,
  sourceArchive,
  sourceEvidence,
  sourceNotices,
} from "./source.mjs";

function checksums(records) {
  return records
    .toSorted((a, b) => compare(a.file, b.file))
    .map((r) => `${r.sha256}  ${r.file}\n`)
    .join("");
}
function record(value) {
  fields(value, ["file", "size", "sha256", "kind"], "artifact record");
  portable(value.file);
  digest(value.sha256);
  check(
    Number.isSafeInteger(value.size) && value.size > 0,
    "invalid artifact size",
  );
  check(
    ["nsis", "dmg", "deb", "evidence", "corresponding-source"].includes(
      value.kind,
    ),
    "unknown artifact kind",
  );
}
export function createCandidate({
  source,
  requested,
  capture,
  bundleRoot,
  output,
  build,
  platform,
  architecture,
  dependencies,
}) {
  source = rootPath(source);
  bundleRoot = rootPath(bundleRoot);
  provenance(build);
  const captured = checkCapture(source, requested, capture);
  const resource = resources(source);
  const notices = sourceNotices(source, captured);
  const deps = dependencies ?? captureDependencies(source);
  validateDependencies(source, deps.inventory);
  const dest = target(platform, architecture);
  check(!resource.pluginRuntime || resource.pluginRuntime.target === dest.triple,
    "packaged plugin runtime target differs from installer target");
  const names = installerNames(requested.version, platform, architecture);
  const expectedSource = `source/Windfall-${requested.commit}.tar`;
  return stage(output, (staging) => {
    const artifacts = [];
    for (const { format, name } of names) {
      const original = `${format}/${name}`;
      fileRecord(bundleRoot, original);
      const file = `installers/${name}`;
      writeNew(staging, file, fs.readFileSync(inside(bundleRoot, original)));
      artifacts.push({ ...fileRecord(staging, file), kind: format });
    }
    writeNew(staging, expectedSource, sourceArchive(source, requested.commit));
    writeNew(staging, "source/inventory.json", json(captured));
    writeNew(staging, "resources.json", json(resource));
    writeNew(staging, "dependencies.json", json(deps.inventory));
    for (const n of notices)
      writeNew(staging, n.output, fs.readFileSync(inside(source, n.file)));
    for (const [n, bytes] of deps.notices) {
      check(
        n.startsWith("notices/dependencies/"),
        "dependency notice outside owned namespace",
      );
      writeNew(staging, n, bytes);
    }
    const dependencyNotices = deps.inventory.packages.flatMap((p) => p.notices);
    const noticeInventory = {
      schemaVersion: 1,
      licenseReview: "pending",
      source: notices,
      dependencies: dependencyNotices,
    };
    writeNew(staging, "notices/inventory.json", json(noticeInventory));
    const nativeFiles = new Set(artifacts.map((a) => a.file));
    for (const file of walk(staging).filter(
      (f) => f !== ".owner" && !nativeFiles.has(f),
    ))
      artifacts.push({
        ...fileRecord(staging, file),
        kind: file === expectedSource ? "corresponding-source" : "evidence",
      });
    artifacts.sort((a, b) => compare(a.file, b.file));
    checkCapture(source, requested, capture);
    const manifest = {
      schemaVersion: 1,
      kind: "unsigned-development-candidate",
      identity: captured.identity,
      target: dest,
      provenance: build,
      source: {
        tree: captured.tree,
        snapshotSha256: captured.snapshotSha256,
        archive: expectedSource,
        inventory: "source/inventory.json",
      },
      security: {
        signature: "not-verified",
        notarization: platform === "macos" ? "not-requested" : "not-applicable",
        distribution: "development-only",
      },
      resources: {
        state: "source-contract-verified",
        inventory: "resources.json",
        helpers: resource.helpers,
        models: "not-admitted",
      },
      dependencies: {
        inventory: "dependencies.json",
        licenseReview: "pending",
        missingProvenance: deps.inventory.packages.filter(
          (p) => p.provenance !== "declared-unreviewed",
        ).length,
      },
      publication: {
        eligible: false,
        blockers: [
          ...BLOCKERS,
          ...(deps.inventory.packages.some(
            (p) => p.provenance !== "declared-unreviewed",
          )
            ? ["missing-dependency-provenance"]
            : []),
        ].sort(compare),
      },
      artifacts,
    };
    writeNew(staging, "manifest.json", json(manifest));
    writeNew(
      staging,
      "SHA256SUMS.txt",
      checksums([...artifacts, fileRecord(staging, "manifest.json")]),
    );
    verifyCandidate({
      source,
      requested,
      candidate: staging,
      internalStaging: true,
    });
  });
}
export function verifyCandidate({
  source,
  requested,
  candidate,
  internalStaging = false,
}) {
  source = rootPath(source);
  candidate = rootPath(candidate);
  const expected = identity(
    requested.version,
    requested.channel,
    requested.commit,
  );
  const m = load(inside(candidate, "manifest.json"));
  fields(
    m,
    [
      "schemaVersion",
      "kind",
      "identity",
      "target",
      "provenance",
      "source",
      "security",
      "resources",
      "dependencies",
      "publication",
      "artifacts",
    ],
    "manifest",
  );
  check(
    m.schemaVersion === 1 && m.kind === "unsigned-development-candidate",
    "unknown manifest schema/kind",
  );
  fields(m.identity, Object.keys(expected), "manifest identity");
  check(
    json(m.identity) === json(expected),
    "manifest version/channel/commit/repository mismatch",
  );
  fields(m.target, ["platform", "architecture", "triple"], "manifest target");
  check(
    json(m.target) === json(target(m.target.platform, m.target.architecture)),
    "target triple mismatch",
  );
  provenance(m.provenance);
  fields(m.security, ["signature", "notarization", "distribution"], "security");
  check(
    m.security.signature === "not-verified" &&
      m.security.distribution === "development-only" &&
      m.security.notarization ===
        (m.target.platform === "macos" ? "not-requested" : "not-applicable"),
    "unknown or contradictory signing/notarization claim",
  );
  fields(
    m.source,
    ["tree", "snapshotSha256", "archive", "inventory"],
    "source manifest",
  );
  check(
    m.source.archive === `source/Windfall-${expected.commit}.tar` &&
      m.source.inventory === "source/inventory.json",
    "corresponding-source contract missing",
  );
  const capture = load(inside(candidate, m.source.inventory));
  const current = checkCapture(source, requested, capture);
  check(
    m.source.tree === current.tree &&
      m.source.snapshotSha256 === current.snapshotSha256,
    "source identity/snapshot mismatch",
  );
  const archive = fs.readFileSync(inside(candidate, m.source.archive));
  check(
    sha256(archive) === sha256(sourceArchive(source, expected.commit)),
    "corresponding-source archive differs from exact commit",
  );
  fields(
    m.resources,
    ["state", "inventory", "helpers", "models"],
    "resource manifest",
  );
  const verifiedResources = resources(source);
  check(!verifiedResources.pluginRuntime || verifiedResources.pluginRuntime.target === m.target.triple,
    "packaged plugin runtime target differs from installer target");
  check(
    json(m.resources) ===
      json({
        state: "source-contract-verified",
        inventory: "resources.json",
        helpers: verifiedResources.helpers,
        models: "not-admitted",
      }),
    "unknown resource contract",
  );
  check(
    json(load(inside(candidate, "resources.json"))) === json(verifiedResources),
    "source resource inventory mismatch",
  );
  fields(
    m.dependencies,
    ["inventory", "licenseReview", "missingProvenance"],
    "dependency manifest",
  );
  check(
    m.dependencies.inventory === "dependencies.json" &&
      m.dependencies.licenseReview === "pending",
    "unsupported dependency compliance claim",
  );
  const deps = load(inside(candidate, "dependencies.json"));
  validateDependencies(source, deps);
  const missing = deps.packages.filter(
    (p) => p.provenance !== "declared-unreviewed",
  ).length;
  check(
    m.dependencies.missingProvenance === missing,
    "dependency provenance count mismatch",
  );
  const notices = load(inside(candidate, "notices/inventory.json"));
  fields(
    notices,
    ["schemaVersion", "licenseReview", "source", "dependencies"],
    "notices inventory",
  );
  check(
    notices.schemaVersion === 1 &&
      notices.licenseReview === "pending" &&
      json(notices.source) === json(sourceNotices(source, current)) &&
      json(notices.dependencies) ===
        json(deps.packages.flatMap((p) => p.notices)),
    "source/dependency notices inventory missing or mismatched",
  );
  for (const n of notices.source)
    check(
      json(fileRecord(candidate, n.output)) ===
        json({ file: n.output, size: n.size, sha256: n.sha256 }),
      "source notice bytes mismatch",
    );
  for (const n of notices.dependencies)
    check(
      json(fileRecord(candidate, n.file)) === json(n),
      "dependency notice bytes mismatch",
    );
  fields(m.publication, ["eligible", "blockers"], "publication gate");
  check(
    m.publication.eligible === false &&
      json(m.publication.blockers) ===
        json(
          [
            ...BLOCKERS,
            ...(missing ? ["missing-dependency-provenance"] : []),
          ].sort(compare),
        ),
    "publication/signing/compliance gates cannot be removed",
  );
  check(
    Array.isArray(m.artifacts) && m.artifacts.length > 0,
    "artifact inventory missing",
  );
  unique(m.artifacts.map((a) => a.file));
  check(
    json(m.artifacts) ===
      json(m.artifacts.toSorted((a, b) => compare(a.file, b.file))),
    "nondeterministic artifact order",
  );
  const required = installerNames(
    expected.version,
    m.target.platform,
    m.target.architecture,
  );
  const native = m.artifacts.filter(
    (a) => !["evidence", "corresponding-source"].includes(a.kind),
  );
  check(
    native.length === required.length &&
      new Set(native.map((a) => a.kind)).size === required.length,
    "missing/duplicate logical platform artifact",
  );
  for (const r of required)
    check(
      native.some(
        (a) => a.kind === r.format && a.file === `installers/${r.name}`,
      ),
      "artifact filename/version/platform mismatch",
    );
  check(
    m.artifacts.filter((a) => a.kind === "corresponding-source").length === 1 &&
      m.artifacts.some(
        (a) => a.kind === "corresponding-source" && a.file === m.source.archive,
      ),
    "corresponding-source artifact missing",
  );
  for (const a of m.artifacts) {
    record(a);
    check(
      a.file !== "manifest.json" && a.file !== "SHA256SUMS.txt",
      "self-referential artifact",
    );
    const actual = fileRecord(candidate, a.file);
    check(
      actual.size === a.size && actual.sha256 === a.sha256,
      "artifact hash/size mismatch",
    );
    check(
      !a.file.startsWith("installers/") || native.includes(a),
      "undeclared installer",
    );
  }
  const all = walk(candidate).filter((n) => !internalStaging || n !== ".owner");
  check(
    json(all) ===
      json(
        [
          ...m.artifacts.map((a) => a.file),
          "manifest.json",
          "SHA256SUMS.txt",
        ].sort(compare),
      ),
    "unlisted/missing candidate files",
  );
  check(
    fs.readFileSync(inside(candidate, "SHA256SUMS.txt"), "utf8") ===
      checksums([...m.artifacts, fileRecord(candidate, "manifest.json")]),
    "checksum file is malformed, stale or mismatched",
  );
  checkCapture(source, requested, capture);
  return m;
}

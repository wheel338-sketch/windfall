import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { validateWorkflow } from "./check-workflow.mjs";
import { ALPHA, identity, installerNames, mode } from "./contracts.mjs";
import { createCandidate, verifyCandidate } from "./candidate.mjs";
import {
  cargoLock,
  dependencyEvidence,
  npmLock,
  validateDependencies,
} from "./dependencies.mjs";
import {
  atomicNew,
  compare,
  fileRecord,
  inside,
  json,
  load,
  parseJson,
  portable,
  rootPath,
  run,
  stage,
  unique,
  walk,
  writeNew,
} from "./files.mjs";
import {
  cargoMetadata,
  githubIdentity,
  resources,
  sourceEvidence,
} from "./source.mjs";

const cli = fileURLToPath(new URL("./cli.mjs", import.meta.url));
const version = "0.2.0-beta.1";
const build = {
  workflowCommit: "a".repeat(40),
  workflowPath: "local",
  runId: "fixture",
  runAttempt: "1",
  runnerImage: "authored-fixture",
  tools: {
    git: "git fixture",
    node: process.version,
    pnpm: "pnpm fixture",
    rustc: "rustc fixture",
    cargo: "cargo fixture",
    tauri: "tauri fixture",
  },
};
function fixture(t, platform = "windows", architecture = "x64") {
  const tempParent = rootPath(os.tmpdir());
  const temp = fs.mkdtempSync(path.join(tempParent, "windfall-release-test-"));
  const owner = randomUUID();
  fs.writeFileSync(path.join(temp, ".test-owner"), owner);
  t.after(() => {
    assert.equal(path.dirname(path.resolve(temp)), tempParent);
    assert.match(path.basename(temp), /^windfall-release-test-/);
    assert.equal(
      fs.readFileSync(path.join(temp, ".test-owner"), "utf8"),
      owner,
    );
    rootPath(temp);
    // Tests that create a junction unlink it explicitly before this owned cleanup.
    fs.rmSync(temp, { recursive: true });
  });
  const source = path.join(temp, "source");
  fs.mkdirSync(source);
  writeNew(
    source,
    "Cargo.toml",
    `[workspace]\nresolver = "3"\nmembers = ["apps/desktop/src-tauri"]\n[workspace.package]\nversion = "${version}"\nedition = "2024"\nlicense = "GPL-3.0-or-later"\n`,
  );
  writeNew(
    source,
    "Cargo.lock",
    `version = 4\n\n[[package]]\nname = "windfall-desktop"\nversion = "${version}"\n`,
  );
  writeNew(
    source,
    "LICENSE",
    "Authored test license fixture; not a distribution notice.\n",
  );
  writeNew(
    source,
    "apps/desktop/package.json",
    json({ name: "windfall-desktop", private: true, version }),
  );
  writeNew(
    source,
    "apps/desktop/pnpm-lock.yaml",
    "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\nimporters:\n  .: {}\npackages:\n\nsnapshots:\n",
  );
  writeNew(
    source,
    "apps/desktop/src-tauri/Cargo.toml",
    '[package]\nname = "windfall-desktop"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\n',
  );
  writeNew(
    source,
    "apps/desktop/src-tauri/src/lib.rs",
    "// Authored source fixture.\npub fn fixture() {}\n",
  );
  writeNew(
    source,
    "apps/desktop/src-tauri/src/main.rs",
    "// Authored desktop entry fixture.\nfn main() {}\n",
  );
  writeNew(
    source,
    "apps/desktop/src-tauri/LICENSE",
    "Authored package notice bytes.\n",
  );
  writeNew(
    source,
    "apps/desktop/src-tauri/tauri.conf.json",
    json({
      productName: "Windfall",
      identifier: "org.windfall.Windfall",
      version,
      bundle: {
        active: true,
        resources: { "../../../content/factory/": "factory/" },
      },
    }),
  );
  writeNew(
    source,
    "crates/windfall-factory/src/lib.rs",
    'static SOUNDS: [Sound; 1] = [sound(Category::Kick, "Fixture", "Fixture.wav", -1.0, fixture)];\n',
  );
  writeNew(
    source,
    "content/factory/Fixture.wav",
    Buffer.from("RIFF actual authored fixture bytes"),
  );
  writeNew(
    source,
    "content/factory/LICENSE.md",
    "CC0 1.0 Universal. Generation code GPL-3.0-or-later. Authored fixture.\n",
  );
  writeNew(
    source,
    "content/factory/README.md",
    "Authored factory provenance fixture.\n",
  );
  run("git", ["init", "--quiet"], source);
  run("git", ["config", "user.name", "Release fixture"], source);
  run("git", ["config", "user.email", "fixture@example.invalid"], source);
  run("git", ["config", "core.autocrlf", "false"], source);
  run("git", ["add", "."], source);
  run(
    "git",
    ["commit", "--quiet", "-m", "Authored future release source fixture"],
    source,
  );
  const requested = {
    commit: run("git", ["rev-parse", "HEAD"], source),
    version,
    channel: "prerelease",
    mode: "unsigned-development",
  };
  const capture = sourceEvidence(source, requested);
  const dependencies = dependencyEvidence(
    source,
    cargoMetadata(source, true),
    {},
    { cargo: [source], npm: [] },
  );
  const bundleRoot = path.join(temp, "bundle");
  fs.mkdirSync(bundleRoot);
  for (const { format, name } of installerNames(
    version,
    platform,
    architecture,
  ))
    writeNew(
      bundleRoot,
      `${format}/${name}`,
      Buffer.from(
        `Authored ${format} artifact bytes; no executable/installability claim.\n`,
      ),
    );
  const options = {
    source,
    requested,
    capture,
    bundleRoot,
    build,
    dependencies,
    platform,
    architecture,
    output: path.join(temp, "candidate"),
  };
  return {
    temp,
    source,
    requested,
    capture,
    dependencies,
    bundleRoot,
    options,
  };
}
function generate(f) {
  return createCandidate(f.options);
}
function graphFixture(t) {
  const f = fixture(t);
  const workspace = path.join(f.source, "Cargo.toml");
  fs.writeFileSync(
    workspace,
    fs
      .readFileSync(workspace, "utf8")
      .replace(
        'members = ["apps/desktop/src-tauri"]',
        'members = ["apps/desktop/src-tauri", "crates/windfall-core"]',
      ),
  );
  writeNew(
    f.source,
    "crates/windfall-core/Cargo.toml",
    '[package]\nname = "windfall-core"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\n',
  );
  writeNew(
    f.source,
    "crates/windfall-core/src/lib.rs",
    "// Authored graph fixture.\n",
  );
  fs.appendFileSync(
    path.join(f.source, "apps/desktop/src-tauri/Cargo.toml"),
    '\n[dependencies]\nwindfall-core = { path = "../../../crates/windfall-core" }\n[features]\ndefault = ["proof"]\nproof = []\n',
  );
  fs.writeFileSync(
    path.join(f.source, "Cargo.lock"),
    `version = 4\n\n[[package]]\nname = "windfall-core"\nversion = "${version}"\n\n[[package]]\nname = "windfall-desktop"\nversion = "${version}"\ndependencies = [\n "windfall-core",\n]\n`,
  );
  run("git", ["add", "."], f.source);
  run(
    "git",
    [
      "commit",
      "--quiet",
      "-m",
      "Authored authoritative dependency graph fixture",
    ],
    f.source,
  );
  f.requested.commit = run("git", ["rev-parse", "HEAD"], f.source);
  f.capture = sourceEvidence(f.source, f.requested);
  f.dependencies = dependencyEvidence(
    f.source,
    cargoMetadata(f.source, true),
    {},
    { cargo: [f.source], npm: [] },
  );
  Object.assign(f.options, {
    requested: f.requested,
    capture: f.capture,
    dependencies: f.dependencies,
  });
  return f;
}
function rewriteDependencyEvidence(f, edit) {
  const p = path.join(f.options.output, "dependencies.json");
  const inventory = load(p);
  edit(inventory);
  fs.writeFileSync(p, json(inventory));
  const manifestPath = path.join(f.options.output, "manifest.json");
  const m = load(manifestPath);
  Object.assign(
    m.artifacts.find((a) => a.file === "dependencies.json"),
    fileRecord(f.options.output, "dependencies.json"),
  );
  fs.writeFileSync(manifestPath, json(m));
  const records = [
    ...m.artifacts,
    fileRecord(f.options.output, "manifest.json"),
  ].sort((a, b) => compare(a.file, b.file));
  fs.writeFileSync(
    path.join(f.options.output, "SHA256SUMS.txt"),
    records.map((r) => `${r.sha256}  ${r.file}\n`).join(""),
  );
}
function verify(f) {
  return verifyCandidate({
    source: f.source,
    requested: f.requested,
    candidate: f.options.output,
  });
}
function mutate(f, edit) {
  const p = path.join(f.options.output, "manifest.json");
  const m = load(p);
  edit(m);
  fs.writeFileSync(p, json(m));
}
function invoke(f, command, extra = []) {
  return spawnSync(
    process.execPath,
    [
      cli,
      command,
      "--source",
      f.source,
      "--commit",
      f.requested.commit,
      "--version",
      f.requested.version,
      "--channel",
      f.requested.channel,
      "--mode",
      f.requested.mode,
      ...extra,
    ],
    { encoding: "utf8", windowsHide: true },
  );
}

test("real generator/verifier and CLI verify succeed, with deterministic repeat bytes", (t) => {
  const f = fixture(t);
  generate(f);
  const m = verify(f);
  assert.equal(m.publication.eligible, false);
  assert.equal(m.security.signature, "not-verified");
  assert.equal(
    invoke(f, "verify", ["--candidate", f.options.output]).status,
    0,
  );
  const again = path.join(f.temp, "candidate-again");
  createCandidate({ ...f.options, output: again });
  assert.deepEqual(walk(again), walk(f.options.output));
  for (const n of walk(again))
    assert.deepEqual(
      fs.readFileSync(inside(again, n)),
      fs.readFileSync(inside(f.options.output, n)),
    );
});
for (const [platform, arch] of [
  ["macos", "aarch64"],
  ["linux", "x64"],
])
  test(`authored ${platform} filename matrix is complete and verified`, (t) => {
    const f = fixture(t, platform, arch);
    generate(f);
    assert.equal(
      verify(f).artifacts.filter((a) => a.file.startsWith("installers/"))
        .length,
      1,
    );
  });
test("hash and size tampering fail through actual CLI exit status", (t) => {
  const f = fixture(t);
  generate(f);
  const m = verify(f);
  const native = m.artifacts.find((a) => a.kind === "nsis");
  const p = path.join(f.options.output, native.file);
  const original = fs.readFileSync(p);
  const changed = Buffer.from(original);
  changed[0] ^= 1;
  fs.writeFileSync(p, changed);
  assert.throws(() => verify(f), /hash\/size/);
  assert.equal(
    invoke(f, "verify", ["--candidate", f.options.output]).status,
    1,
  );
  fs.writeFileSync(p, Buffer.concat([original, Buffer.from("more")]));
  assert.throws(() => verify(f), /hash\/size/);
});
test("immutable alpha, semantic channels and full source identity are enforced", () => {
  assert.throws(
    () => identity(ALPHA.version, "prerelease", "a".repeat(40)),
    /immutable/,
  );
  assert.throws(
    () => identity(version, "prerelease", ALPHA.commit),
    /immutable/,
  );
  for (const v of [
    "01.2.3",
    "1.2",
    "1.2.3+build",
    "1.2.3-beta.01",
    "../1.2.3",
    "1.2.3-",
  ])
    assert.throws(() => identity(v, "prerelease", "a".repeat(40)));
  assert.throws(() => identity(version, "stable", "a".repeat(40)), /channel/);
  assert.throws(
    () => identity("1.2.3", "prerelease", "a".repeat(40)),
    /channel/,
  );
  assert.throws(() => identity(version, "nightly", "a".repeat(40)), /channel/);
  assert.throws(() => identity(version, "prerelease", "a".repeat(7)), /SHA/);
  assert.equal(identity("1.2.3", "stable", "a".repeat(40)).channel, "stable");
  for (const v of ["0.0.9-beta.1", "0.1.0-alpha.0", "0.1.0-ALPHA.9"])
    assert.throws(() => identity(v, "prerelease", "a".repeat(40)), /newer/);
  assert.equal(
    identity("0.1.0-alpha.2", "prerelease", "a".repeat(40)).version,
    "0.1.0-alpha.2",
  );
});
test("source metadata mismatches and post-capture changes cannot produce a candidate", (t) => {
  const f = fixture(t);
  assert.throws(
    () => sourceEvidence(f.source, { ...f.requested, commit: "b".repeat(40) }),
    /commit mismatch/,
  );
  assert.throws(
    () => sourceEvidence(f.source, { ...f.requested, version: "0.3.0-beta.1" }),
    /versions disagree/,
  );
  fs.appendFileSync(
    path.join(f.source, "apps/desktop/src-tauri/src/lib.rs"),
    "// Changed after capture\n",
  );
  assert.throws(() => generate(f), /clean/);
  assert.equal(fs.existsSync(f.options.output), false);
});
test("source changes committed after capture are rejected even with relabelled expected commit", (t) => {
  const f = fixture(t);
  fs.appendFileSync(path.join(f.source, "LICENSE"), "change\n");
  run("git", ["add", "."], f.source);
  run("git", ["commit", "--quiet", "-m", "source moved"], f.source);
  f.requested.commit = run("git", ["rev-parse", "HEAD"], f.source);
  assert.throws(() => generate(f), /source changed/);
});
test("Git assume-unchanged cannot conceal source tampering from immutable blob checks", (t) => {
  const f = fixture(t);
  const file = "apps/desktop/src-tauri/src/lib.rs";
  run("git", ["update-index", "--assume-unchanged", file], f.source);
  fs.appendFileSync(
    path.join(f.source, file),
    "// concealed source mutation\n",
  );
  assert.equal(run("git", ["status", "--porcelain"], f.source), "");
  assert.throws(() => generate(f), /immutable commit/);
});
test("source archive cannot omit or rewrite files with export attributes", (t) => {
  const f = fixture(t);
  writeNew(f.source, ".gitattributes", "LICENSE export-ignore\n");
  run("git", ["add", "."], f.source);
  run("git", ["commit", "--quiet", "-m", "archive omission fixture"], f.source);
  assert.throws(
    () =>
      sourceEvidence(f.source, {
        ...f.requested,
        commit: run("git", ["rev-parse", "HEAD"], f.source),
      }),
    /export-ignore\/export-subst/,
  );
});
test("Git-confirmed Windows CRLF checkout is captured without mislabelling its byte hash", (t) => {
  const f = fixture(t);
  writeNew(
    f.source,
    ".gitattributes",
    "apps/desktop/src-tauri/src/lib.rs text eol=crlf\n",
  );
  run("git", ["add", "."], f.source);
  run(
    "git",
    ["commit", "--quiet", "-m", "Explicit checkout line-ending fixture"],
    f.source,
  );
  f.requested.commit = run("git", ["rev-parse", "HEAD"], f.source);
  f.capture = sourceEvidence(f.source, f.requested);
  const p = path.join(f.source, "apps/desktop/src-tauri/src/lib.rs");
  fs.writeFileSync(
    p,
    fs
      .readFileSync(p, "utf8")
      .replaceAll("\r\n", "\n")
      .replaceAll("\n", "\r\n"),
  );
  assert.equal(
    run("git", ["diff", "--no-ext-diff", "HEAD", "--"], f.source),
    "",
  );
  const fresh = sourceEvidence(f.source, f.requested);
  assert.notEqual(fresh.snapshotSha256, f.capture.snapshotSha256);
  createCandidate({ ...f.options, capture: fresh });
  assert.equal(verify(f).identity.commit, f.requested.commit);
});
test("each Cargo member version is validated, including independent package versions", (t) => {
  const f = fixture(t);
  const p = path.join(f.source, "apps/desktop/src-tauri/Cargo.toml");
  fs.writeFileSync(
    p,
    fs
      .readFileSync(p, "utf8")
      .replace("version.workspace = true", 'version = "0.7.0"'),
  );
  fs.writeFileSync(
    path.join(f.source, "Cargo.lock"),
    'version = 4\n[[package]]\nname = "windfall-desktop"\nversion = "0.7.0"\n',
  );
  run("git", ["add", "."], f.source);
  run("git", ["commit", "--quiet", "-m", "mismatch fixture"], f.source);
  assert.throws(
    () =>
      sourceEvidence(f.source, {
        ...f.requested,
        commit: run("git", ["rev-parse", "HEAD"], f.source),
      }),
    /member version/,
  );
});
test("CLI capture is exclusive and invalid options have nonzero status", (t) => {
  const f = fixture(t);
  const output = path.join(f.temp, "capture.json");
  assert.equal(invoke(f, "capture", ["--output", output]).status, 0);
  const bytes = fs.readFileSync(output);
  assert.equal(invoke(f, "capture", ["--output", output]).status, 1);
  assert.deepEqual(fs.readFileSync(output), bytes);
  assert.equal(invoke(f, "preflight", ["--unknown", "bad"]).status, 1);
  assert.equal(invoke(f, "preflight", ["--github", "--github"]).status, 1);
});
test("signed requests fail closed both without and with named operator material", () => {
  assert.throws(
    () => mode("signed-release"),
    /requires operator configuration/,
  );
  assert.throws(
    () =>
      mode("signed-release", {
        WINDFALL_SIGNING_PROFILE: "configured",
        WINDFALL_SIGNING_VERIFIER: "configured",
      }),
    /not implemented/,
  );
  assert.throws(() => mode("production"), /unknown/);
});
test("traversal, absolute paths, Windows names, collisions and symlink escapes are rejected", (t) => {
  const f = fixture(t);
  for (const name of [
    "../outside",
    "/tmp/file",
    "C:/outside",
    "a\\b",
    "a/../b",
    "./a",
    "a//b",
    "CON.txt",
    "a/file.",
    "a/file ",
    "a:b",
    "a\u0000b",
  ])
    assert.throws(() => portable(name));
  assert.throws(() => unique(["x/File.exe", "x/file.exe"]), /case-colliding/);
  const link = path.join(f.bundleRoot, "escaped");
  fs.symlinkSync(
    f.source,
    link,
    process.platform === "win32" ? "junction" : "dir",
  );
  try {
    assert.throws(() => inside(f.bundleRoot, "escaped/LICENSE"), /symlink/);
    assert.throws(() => walk(f.bundleRoot), /symlink/);
  } finally {
    fs.unlinkSync(link);
  }
});
test("staging never overwrites an existing candidate or competitor file", (t) => {
  const f = fixture(t);
  fs.mkdirSync(f.options.output);
  writeNew(f.options.output, "sentinel", "competitor bytes");
  assert.throws(() => generate(f), /clobber/);
  assert.equal(
    fs.readFileSync(path.join(f.options.output, "sentinel"), "utf8"),
    "competitor bytes",
  );
  const out = path.join(f.temp, "contended");
  assert.throws(
    () =>
      stage(out, (s) => {
        writeNew(s, "data", "owned");
        fs.mkdirSync(out);
        writeNew(out, "sentinel", "racing competitor");
      }),
    /EEXIST/,
  );
  assert.equal(
    fs.readFileSync(path.join(out, "sentinel"), "utf8"),
    "racing competitor",
  );
  assert.equal(
    fs.readdirSync(f.temp).some((n) => n.startsWith(".release-stage-")),
    false,
  );
  const atomic = path.join(f.temp, "final.txt");
  atomicNew(atomic, "first");
  assert.throws(() => atomicNew(atomic, "second"), /EEXIST/);
  assert.equal(fs.readFileSync(atomic, "utf8"), "first");
});
for (const [label, edit] of [
  ["unknown signature", (m) => (m.security.signature = "probably-signed")],
  ["contradictory signed claim", (m) => (m.security.signature = "verified")],
  ["contradictory notarization", (m) => (m.security.notarization = "verified")],
  ["missing signing state", (m) => delete m.security.signature],
  ["missing source", (m) => delete m.source],
  ["mismatched commit", (m) => (m.identity.commit = "d".repeat(40))],
  ["mismatched version", (m) => (m.identity.version = "0.9.0-beta.1")],
  [
    "duplicate logical artifact",
    (m) =>
      m.artifacts.push({
        ...m.artifacts.find((a) => a.kind === "nsis"),
        file: "installers/duplicate.exe",
      }),
  ],
  [
    "case collision",
    (m) =>
      m.artifacts.push({
        ...m.artifacts[0],
        file: m.artifacts[0].file.toUpperCase(),
      }),
  ],
  ["path traversal", (m) => (m.artifacts[0].file = "../outside")],
  [
    "wrong filename",
    (m) =>
      (m.artifacts.find((a) => a.kind === "nsis").file =
        "installers/Windfall_0.9.0_x64-setup.exe"),
  ],
  ["unknown manifest field", (m) => (m.productionSigned = true)],
  ["truncated provenance", (m) => delete m.provenance.tools.cargo],
  ["publication claim", (m) => (m.publication.eligible = true)],
])
  test(`verifier rejects ${label}`, (t) => {
    const f = fixture(t);
    generate(f);
    mutate(f, edit);
    assert.throws(() => verify(f));
  });
test("source/notices/resources/checksums require actual bytes and complete inventories", (t) => {
  const f = fixture(t);
  generate(f);
  const sourceTar = path.join(
    f.options.output,
    `source/Windfall-${f.requested.commit}.tar`,
  );
  const original = fs.readFileSync(sourceTar);
  fs.writeFileSync(sourceTar, "wrong exact-source archive");
  assert.throws(() => verify(f), /source archive/);
  fs.writeFileSync(sourceTar, original);
  const notice = path.join(f.options.output, "notices/source/LICENSE");
  fs.unlinkSync(notice);
  assert.throws(() => verify(f), /ENOENT/);
});
test("extra artifacts and truncated/malformed JSON or checksums cannot pass", (t) => {
  const f = fixture(t);
  generate(f);
  writeNew(f.options.output, "extra.exe", "unlisted");
  assert.throws(() => verify(f), /unlisted/);
  fs.unlinkSync(path.join(f.options.output, "extra.exe"));
  const sums = path.join(f.options.output, "SHA256SUMS.txt");
  fs.writeFileSync(sums, "truncated\n");
  assert.throws(() => verify(f), /checksum/);
  fs.writeFileSync(
    path.join(f.options.output, "manifest.json"),
    '{"schemaVersion":',
  );
  assert.throws(() => verify(f), /JSON/);
});
test("duplicate JSON metadata fields and duplicate workflow permissions fail closed", (t) => {
  assert.throws(
    () => parseJson('{"security":"signed","security":"unsigned"}'),
    /duplicate JSON/,
  );
  const f = fixture(t);
  const file = path.join(f.temp, "duplicate-workflow.yml");
  const original = fs.readFileSync(
    fileURLToPath(
      new URL("../../.github/workflows/release-candidate.yml", import.meta.url),
    ),
    "utf8",
  );
  const duplicate = original.replace(
    /"contents":\s*"read"/,
    '"contents": "write", "contents": "read"',
  );
  assert.notEqual(duplicate, original);
  fs.writeFileSync(file, duplicate);
  assert.throws(() => validateWorkflow(file), /duplicate JSON/);
  assert.doesNotThrow(() => validateWorkflow());
});
test("unaccepted helper/model/resource mappings are gated", (t) => {
  const f = fixture(t);
  const p = path.join(f.source, "apps/desktop/src-tauri/tauri.conf.json");
  const conf = load(p);
  conf.bundle.externalBin = ["unavailable-helper"];
  fs.writeFileSync(p, json(conf));
  assert.throws(() => resources(f.source), /helper/);
  delete conf.bundle.externalBin;
  conf.bundle.resources["model.bin"] = "model.bin";
  fs.writeFileSync(p, json(conf));
  assert.throws(() => resources(f.source), /resource mapping/);
});
test("export-owned runtime resource inventory refuses missing helper, ABI/version and changed bytes", (t) => {
  const f = fixture(t);
  const config = path.join(f.source, "apps/desktop/src-tauri/tauri.conf.json");
  const conf = load(config);
  conf.bundle.resources["../../../target/release/plugin-runtime/"] = "plugin-runtime/";
  fs.writeFileSync(config, json(conf));
  const directory = path.join(f.source, "target/release/plugin-runtime");
  fs.mkdirSync(directory, { recursive: true });
  const version = /^version\s*=\s*"([^"]+)"/m.exec(fs.readFileSync(path.join(f.source, "Cargo.toml"), "utf8"))[1];
  const files = [{ name: "windfall_plugin.dll", bytes: "owned runtime" }, { name: "windfall-plugin-audio.exe", bytes: "owned helper" }];
  const manifest = { abi: 1, version, target: "x86_64-pc-windows-msvc", files: files.map(f => ({ name: f.name, sha256: sha256(Buffer.from(f.bytes)) })) };
  for (const file of files) fs.writeFileSync(path.join(directory, file.name), file.bytes);
  const manifestPath = path.join(directory, "distribution.json"); fs.writeFileSync(manifestPath, json(manifest));
  assert.equal(resources(f.source).helpers, "export-owned-audio-helper");
  fs.unlinkSync(path.join(directory, files[1].name));
  assert.throws(() => resources(f.source), /missing\/unlisted/);
  fs.writeFileSync(path.join(directory, files[1].name), files[1].bytes);
  for (const bad of [{ ...manifest, abi: 2 }, { ...manifest, version: "0.0.0-wrong" }]) {
    fs.writeFileSync(manifestPath, json(bad)); assert.throws(() => resources(f.source), /ABI\/version\/target/);
  }
  fs.writeFileSync(manifestPath, json(manifest));
  fs.writeFileSync(path.join(directory, files[0].name), "changed runtime");
  assert.throws(() => resources(f.source), /checksum mismatch/);
});
test("all pinned Tauri platform JSON/JSON5/TOML overrides are refused even when ignored", (t) => {
  const f = fixture(t);
  assert.equal(resources(f.source).helpers, "not-admitted");
  writeNew(
    f.source,
    ".gitignore",
    "apps/desktop/src-tauri/*.json5\napps/desktop/src-tauri/*.toml\napps/desktop/src-tauri/tauri.*.conf.json\n",
  );
  run("git", ["add", ".gitignore"], f.source);
  run(
    "git",
    ["commit", "--quiet", "-m", "Authored ignored config fixture"],
    f.source,
  );
  f.requested.commit = run("git", ["rev-parse", "HEAD"], f.source);
  for (const platform of ["windows", "macos", "linux", "android", "ios"]) {
    for (const name of [
      `tauri.${platform}.conf.json5`,
      `Tauri.${platform}.toml`,
      `tauri.${platform}.conf.json`,
    ]) {
      const file = path.join(f.source, "apps/desktop/src-tauri", name);
      fs.writeFileSync(
        file,
        name.endsWith("toml")
          ? '[bundle]\nexternalBin = ["unavailable-helper"]\n'
          : '{"bundle":{"resources":{"model.bin":"model.bin"}}}\n',
      );
      assert.throws(() => resources(f.source), /platform.*config/);
      if (name === "tauri.windows.conf.json5") {
        const result = invoke(f, "preflight");
        assert.equal(result.status, 1);
        assert.match(result.stderr, /platform.*config/);
      }
      fs.unlinkSync(file);
    }
  }
  for (const name of ["tauri.conf.json5", "Tauri.toml"]) {
    const file = path.join(f.source, "apps/desktop/src-tauri", name);
    fs.writeFileSync(file, "Authored refused alternate config.");
    assert.throws(() => resources(f.source), /alternate config/);
    fs.unlinkSync(file);
  }
  resources(f.source);
});
test("SBOM changed GPL declaration fails against authoritative Cargo metadata", (t) => {
  const f = graphFixture(t);
  const inventory = structuredClone(f.dependencies.inventory);
  inventory.packages.find(
    (p) => p.name === "windfall-desktop",
  ).declaredLicense = "MIT";
  assert.throws(
    () => validateDependencies(f.source, inventory),
    /authoritative/,
  );
});
test("SBOM missing desktop-to-core edge fails against authoritative Cargo metadata", (t) => {
  const f = graphFixture(t);
  const inventory = structuredClone(f.dependencies.inventory);
  const desktop = inventory.cargoGraph.find((p) =>
    p.package.includes("windfall-desktop@"),
  );
  assert.equal(desktop.dependencies.length, 1);
  desktop.dependencies = [];
  assert.throws(
    () => validateDependencies(f.source, inventory),
    /authoritative/,
  );
});
test("offline metadata inputs cannot bootstrap forged licenses, edges, features or workspace nodes", (t) => {
  const f = graphFixture(t);
  const meta = cargoMetadata(f.source, true);
  for (const edit of [
    (m) => {
      m.packages[0].license = "MIT";
    },
    (m) => {
      m.resolve.nodes.find((n) => n.dependencies.length).dependencies = [];
    },
    (m) => {
      m.resolve.nodes[0].dependencies.push(m.resolve.nodes[0].id);
    },
    (m) => {
      m.resolve.nodes[0].features = ["fictional"];
    },
    (m) => {
      m.resolve.nodes.find((n) => n.deps.length).deps[0].dep_kinds[0].target =
        "cfg(windows)";
    },
    (m) => {
      m.workspace_members.pop();
    },
    (m) => {
      m.packages.pop();
    },
  ]) {
    const changed = structuredClone(meta);
    edit(changed);
    assert.throws(
      () =>
        dependencyEvidence(
          f.source,
          changed,
          {},
          { cargo: [f.source], npm: [] },
        ),
      /authoritative/,
    );
  }
});
for (const [label, edit] of [
  [
    "license",
    (i) => {
      i.packages.find((p) => p.name === "windfall-desktop").declaredLicense =
        "MIT";
    },
  ],
  [
    "missing edge",
    (i) => {
      i.cargoGraph.find((n) => n.dependencies.length).dependencies = [];
    },
  ],
  [
    "false edge",
    (i) => {
      const core = i.cargoGraph.find((n) =>
        n.package.includes("windfall-core@"),
      );
      core.dependencies.push(
        i.cargoGraph.find((n) => n.package.includes("windfall-desktop@"))
          .package,
      );
    },
  ],
  [
    "features",
    (i) => {
      i.cargoGraph.find((n) => n.features.length).features = [];
    },
  ],
  [
    "platform",
    (i) => {
      i.cargoGraph.find((n) => n.edges.length).edges[0].kinds[0].target =
        "cfg(windows)";
    },
  ],
  [
    "dependency kind",
    (i) => {
      i.cargoGraph.find((n) => n.edges.length).edges[0].kinds[0].kind = "build";
    },
  ],
])
  test(`independent verifier rejects forged SBOM ${label} after artifact hashes/checksums are updated`, (t) => {
    const f = graphFixture(t);
    generate(f);
    rewriteDependencyEvidence(f, edit);
    assert.throws(() => verify(f), /authoritative/);
    const result = invoke(f, "verify", ["--candidate", f.options.output]);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /authoritative/);
  });
test("additional auto-bundled desktop binaries require an accepted helper contract", (t) => {
  const f = fixture(t);
  fs.appendFileSync(
    path.join(f.source, "apps/desktop/src-tauri/Cargo.toml"),
    '\n[[bin]]\nname = "windfall-desktop"\npath = "src/main.rs"\n[[bin]]\nname = "unaccepted-helper"\npath = "src/helper.rs"\n',
  );
  writeNew(f.source, "apps/desktop/src-tauri/src/helper.rs", "fn main() {}\n");
  run("git", ["add", "."], f.source);
  run(
    "git",
    ["commit", "--quiet", "-m", "unaccepted helper fixture"],
    f.source,
  );
  assert.throws(
    () =>
      sourceEvidence(f.source, {
        ...f.requested,
        commit: run("git", ["rev-parse", "HEAD"], f.source),
      }),
    /binary\/helper/,
  );
});
test("dependency capture is lock backed and missing provenance remains an explicit gate", (t) => {
  const f = fixture(t);
  const packageManifest = path.join(
    f.source,
    "apps/desktop/src-tauri/Cargo.toml",
  );
  fs.writeFileSync(
    packageManifest,
    fs
      .readFileSync(packageManifest, "utf8")
      .replace("license.workspace = true\n", ""),
  );
  run("git", ["add", "."], f.source);
  run(
    "git",
    [
      "commit",
      "--quiet",
      "-m",
      "Authored package without a license declaration",
    ],
    f.source,
  );
  f.requested.commit = run("git", ["rev-parse", "HEAD"], f.source);
  f.capture = sourceEvidence(f.source, f.requested);
  f.dependencies = dependencyEvidence(
    f.source,
    cargoMetadata(f.source, true),
    {},
    { cargo: [f.source], npm: [] },
  );
  const inv = structuredClone(f.dependencies.inventory);
  assert.equal(inv.packages[0].declaredLicense, null);
  assert.equal(inv.packages[0].provenance, "missing-license-or-notice");
  createCandidate({
    ...f.options,
    requested: f.requested,
    capture: f.capture,
    dependencies: { ...f.dependencies, inventory: inv },
  });
  const m = verify(f);
  assert.equal(m.dependencies.missingProvenance, 1);
  assert.ok(m.publication.blockers.includes("missing-dependency-provenance"));
  inv.packages[0].provenance = "declared-unreviewed";
  assert.throws(() => validateDependencies(f.source, inv), /contradictory/);
});
test("malformed/truncated/duplicate dependency evidence and integrity are rejected", (t) => {
  const f = fixture(t);
  const meta = cargoMetadata(f.source, true);
  assert.throws(
    () =>
      dependencyEvidence(
        f.source,
        { ...meta, resolve: null },
        {},
        { cargo: [f.source], npm: [] },
      ),
    /authoritative/,
  );
  assert.throws(
    () =>
      dependencyEvidence(
        f.source,
        meta,
        { MIT: "bad" },
        { cargo: [f.source], npm: [] },
      ),
    /group/,
  );
  assert.throws(
    () =>
      dependencyEvidence(
        f.source,
        { ...meta, packages: [...meta.packages, meta.packages[0]] },
        {},
        { cargo: [f.source], npm: [] },
      ),
    /authoritative/,
  );
  const inv = structuredClone(f.dependencies.inventory);
  inv.packages = [];
  assert.throws(() => validateDependencies(f.source, inv), /truncated/);
  assert.throws(
    () => cargoLock('version = 4\n[[package]]\nname = "broken"'),
    /version/,
  );
  assert.throws(
    () =>
      npmLock(
        "lockfileVersion: '9.0'\npackages:\n  x@1.0.0:\n    resolution: {}\n",
      ),
    /malformed/,
  );
  const text =
    "lockfileVersion: '9.0'\nsettings:\nimporters:\npackages:\n  x@1.0.0:\n    resolution: {integrity: sha512-" +
    Buffer.alloc(64).toString("base64") +
    "}\nsnapshots:\n";
  assert.equal(npmLock(text)[0].name, "x");
  assert.throws(
    () => npmLock(text.replace("integrity:", "tarball:")),
    /integrity/,
  );
});
test("npm provenance uses actual installed declarations, pinned integrity and notice bytes", (t) => {
  const f = fixture(t);
  const integrity = `sha512-${Buffer.alloc(64, 42).toString("base64")}`;
  const lock =
    "lockfileVersion: '9.0'\nsettings:\nimporters:\npackages:\n  x@1.0.0:\n    resolution: {integrity: " +
    integrity +
    "}\nsnapshots:\n";
  fs.writeFileSync(path.join(f.source, "apps/desktop/pnpm-lock.yaml"), lock);
  const packageRoot = path.join(
    f.source,
    "apps/desktop/node_modules/.pnpm/x@1.0.0/node_modules/x",
  );
  fs.mkdirSync(packageRoot, { recursive: true });
  writeNew(
    packageRoot,
    "package.json",
    json({ name: "x", version: "1.0.0", license: "Apache-2.0" }),
  );
  writeNew(
    packageRoot,
    "LICENSE",
    "Actual authored dependency notice bytes.\n",
  );
  const peerRoot = path.join(
    f.source,
    "apps/desktop/node_modules/.pnpm/x@1.0.0_peer/node_modules/x",
  );
  const npmStore = path.join(f.source, "apps/desktop/node_modules");
  fs.mkdirSync(peerRoot, { recursive: true });
  writeNew(
    peerRoot,
    "package.json",
    fs.readFileSync(path.join(packageRoot, "package.json")),
  );
  writeNew(
    peerRoot,
    "LICENSE",
    fs.readFileSync(path.join(packageRoot, "LICENSE")),
  );
  const meta = cargoMetadata(f.source, true);
  const groups = {
    MIT: [
      {
        name: "x",
        versions: ["1.0.0"],
        paths: [packageRoot, peerRoot],
        license: "MIT",
      },
    ],
  };
  const ev = dependencyEvidence(f.source, meta, groups, {
    cargo: [f.source],
    npm: [npmStore],
  });
  const npm = ev.inventory.packages.find((p) => p.ecosystem === "npm");
  assert.equal(npm.declaredLicense, "Apache-2.0");
  assert.equal(npm.integrity, integrity);
  assert.deepEqual(
    ev.notices.get(npm.notices[0].file),
    fs.readFileSync(path.join(packageRoot, "LICENSE")),
  );
  const changed = structuredClone(ev.inventory);
  changed.packages.find((p) => p.ecosystem === "npm").declaredLicense = "MIT";
  assert.throws(() => validateDependencies(f.source, changed), /authoritative/);
  const omitted = structuredClone(ev.inventory);
  const omittedPackage = omitted.packages.find((p) => p.ecosystem === "npm");
  omittedPackage.declaredLicense = null;
  omittedPackage.provenance = "not-installed-on-this-platform";
  omittedPackage.notices = [];
  assert.throws(() => validateDependencies(f.source, omitted), /authoritative/);
  assert.throws(
    () =>
      dependencyEvidence(f.source, meta, groups, {
        cargo: [f.source],
        npm: [path.join(f.source, "apps/desktop/src-tauri")],
      }),
    /outside allowed/,
  );
  fs.writeFileSync(
    path.join(peerRoot, "package.json"),
    json({ name: "x", version: "1.0.0", license: "MIT" }),
  );
  assert.throws(
    () =>
      dependencyEvidence(f.source, meta, groups, {
        cargo: [f.source],
        npm: [npmStore],
      }),
    /conflicting/,
  );
  fs.writeFileSync(
    path.join(peerRoot, "package.json"),
    fs.readFileSync(path.join(packageRoot, "package.json")),
  );
  fs.writeFileSync(
    path.join(peerRoot, "LICENSE"),
    "Conflicting authored peer notice.\n",
  );
  assert.throws(
    () =>
      dependencyEvidence(f.source, meta, groups, {
        cargo: [f.source],
        npm: [npmStore],
      }),
    /conflicting/,
  );
  fs.writeFileSync(
    path.join(peerRoot, "LICENSE"),
    fs.readFileSync(path.join(packageRoot, "LICENSE")),
  );
  groups.MIT[0].versions = ["9.0.0"];
  assert.throws(
    () =>
      dependencyEvidence(f.source, meta, groups, {
        cargo: [f.source],
        npm: [npmStore],
      }),
    /identity\/metadata mismatch/,
  );
});
test("GitHub preflight preserves alpha and rejects already published/draft/tag identities", async () => {
  const original = globalThis.fetch;
  let reused = false;
  let changed = false;
  globalThis.fetch = async (url) => {
    const route = String(url).split("/erivgout/windfall/")[1];
    const body =
      route === `git/ref/tags/${ALPHA.tag}`
        ? {
            object: {
              type: "tag",
              sha: changed ? "f".repeat(40) : ALPHA.tagObject,
            },
          }
        : route === `git/tags/${ALPHA.tagObject}`
          ? { object: { type: "commit", sha: ALPHA.commit } }
          : route === `releases/tags/${ALPHA.tag}`
            ? {
                tag_name: ALPHA.tag,
                draft: false,
                prerelease: true,
                assets: ALPHA.assets,
              }
            : route.startsWith("git/matching-refs")
              ? []
              : reused
                ? [{ tag_name: `v${version}`, draft: true }]
                : [];
    return { ok: true, json: async () => body };
  };
  try {
    const req = { version, channel: "prerelease", commit: "b".repeat(40) };
    assert.equal(
      (await githubIdentity(req, "synthetic-token-never-printed")).state,
      "unpublished-identity-observed",
    );
    reused = true;
    await assert.rejects(githubIdentity(req, "synthetic"), /already exists/);
    reused = false;
    changed = true;
    await assert.rejects(githubIdentity(req, "synthetic"), /alpha tag changed/);
  } finally {
    globalThis.fetch = original;
  }
});

#!/usr/bin/env node
// Writes the static update manifest (`latest.json`) that Windfall's updater
// reads from the latest GitHub release, for the Windows x64 NSIS installer.
// The installer's signature is verified here first, against the public key
// the app embeds, so a manifest that the app would refuse is never written.
//
//   node scripts/release/update-manifest.mjs \
//     --installer target/release/bundle/nsis/Windfall_1.0.1_x64-setup.exe \
//     --notes-file docs/RELEASE-NOTES-1.0.1.md
//
// Options: --signature (default: the installer path + .sig), --version
// (default: tauri.conf.json), --url (default: the GitHub release asset),
// --public-key (a .pub file; default: tauri.conf.json), --pub-date (ISO 8601;
// default: now), --output (default: latest.json beside the installer).
import fs from "node:fs";
import path from "node:path";
import { createHash, createPublicKey, verify } from "node:crypto";
import { fileURLToPath } from "node:url";

export const REPOSITORY = "wheel338-sketch/windfall";
export const PLATFORM = "windows-x86_64";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function fail(message) {
  throw new Error(`update-manifest: ${message}`);
}

/** The lines of a minisign file that Tauri stores base64-encoded (.sig, .pub, pubkey). */
function minisignLines(encoded, label) {
  const text = Buffer.from(encoded.trim(), "base64").toString("utf8");
  const lines = text.split(/\r?\n/);
  if (!lines[0]?.startsWith("untrusted comment:")) fail(`${label} is not a base64 minisign file`);
  return lines;
}

/** The public key's id and Ed25519 key, from the base64 of a minisign public key file. */
export function readPublicKey(encoded) {
  const raw = Buffer.from(minisignLines(encoded, "public key")[1] ?? "", "base64");
  if (raw.length !== 42 || raw.toString("latin1", 0, 2) !== "Ed") fail("public key is not an Ed25519 minisign key");
  const key = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: raw.subarray(10).toString("base64url") }, format: "jwk" });
  return { id: raw.subarray(2, 10), key };
}

/**
 * Checks a Tauri updater signature the way the updater does: the key id, the
 * Ed25519 signature over the file (BLAKE2b-512 prehashed for "ED"), the global
 * signature over the trusted comment, and the version and file name the
 * comment names. Returns the trusted comment's fields.
 */
export function verifySignature({ data, signature, publicKey, version, file }) {
  const lines = minisignLines(signature, "signature");
  const raw = Buffer.from(lines[1] ?? "", "base64");
  const comment = lines[2]?.startsWith("trusted comment: ") ? lines[2].slice("trusted comment: ".length) : fail("signature has no trusted comment");
  const global = Buffer.from(lines[3] ?? "", "base64");
  if (raw.length !== 74 || global.length !== 64) fail("signature is malformed");
  const algorithm = raw.toString("latin1", 0, 2);
  if (algorithm !== "ED" && algorithm !== "Ed") fail("signature algorithm is not Ed25519");
  const { id, key } = readPublicKey(publicKey);
  if (!raw.subarray(2, 10).equals(id)) fail("signature was made with a different key");
  const signed = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (!verify(null, signed, key, raw.subarray(10))) fail("signature does not match the file");
  if (!verify(null, Buffer.concat([raw.subarray(10), Buffer.from(comment, "utf8")]), key, global)) fail("trusted comment is not signed");
  const fields = Object.fromEntries(comment.split("\t").map((field) => {
    const at = field.indexOf(":");
    return at < 0 ? [field, ""] : [field.slice(0, at), field.slice(at + 1)];
  }));
  if (fields.version !== version) fail(`signature is for version ${fields.version ?? "(none)"}, not ${version}`);
  if (file !== undefined && fields.file !== file) fail(`signature is for ${fields.file ?? "(no file)"}, not ${file}`);
  return fields;
}

/** The project format the app reads, which the updater requires a release to declare. */
export function projectFormatVersion(source = fs.readFileSync(path.join(root, "crates/windfall-project/src/model.rs"), "utf8")) {
  const match = /pub const FORMAT_VERSION: u32 = (\d+);/.exec(source);
  return match ? Number(match[1]) : fail("FORMAT_VERSION not found");
}

export function releaseAssetUrl(version, name) {
  return `https://github.com/${REPOSITORY}/releases/download/v${version}/${encodeURIComponent(name)}`;
}

/** Tauri's static update manifest, plus the channel and format Windfall's policy checks. */
export function updateManifest({ version, notes, pubDate, signature, url, format }) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) fail(`${version} is not a stable release version (there is no preview channel)`);
  if (Number.isNaN(Date.parse(pubDate))) fail("pub_date is not a date");
  return {
    version,
    notes,
    pub_date: new Date(pubDate).toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: { [PLATFORM]: { signature: signature.trim(), url } },
    windfall: { channel: "stable", projectFormatVersion: format },
  };
}

function options(argv) {
  const allowed = ["installer", "signature", "notes-file", "version", "url", "public-key", "pub-date", "output"];
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index]?.slice(2);
    if (!argv[index]?.startsWith("--") || !allowed.includes(name) || name in values) fail(`unknown or repeated option ${argv[index]}`);
    if (argv[index + 1] === undefined) fail(`--${name} needs a value`);
    values[name] = argv[index + 1];
  }
  for (const name of ["installer", "notes-file"]) if (!values[name]) fail(`--${name} is required`);
  return values;
}

export function main(argv) {
  const values = options(argv);
  const config = JSON.parse(fs.readFileSync(path.join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  const version = values.version ?? config.version;
  const installer = path.resolve(values.installer);
  const name = path.basename(installer);
  if (!name.endsWith("-setup.exe") || !name.includes(`_${version}_`)) fail(`${name} is not the ${version} NSIS installer`);
  const signature = fs.readFileSync(values.signature ?? `${installer}.sig`, "utf8");
  const publicKey = values["public-key"] ? fs.readFileSync(values["public-key"], "utf8") : config.plugins.updater.pubkey;
  const fields = verifySignature({ data: fs.readFileSync(installer), signature, publicKey, version, file: name });
  const manifest = updateManifest({
    version,
    notes: fs.readFileSync(values["notes-file"], "utf8").trim(),
    pubDate: values["pub-date"] ?? new Date().toISOString(),
    signature,
    url: values.url ?? releaseAssetUrl(version, name),
    format: projectFormatVersion(),
  });
  const output = path.resolve(values.output ?? path.join(path.dirname(installer), "latest.json"));
  fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Signature verified (${fields.file}, version ${fields.version}).`);
  console.log(`Wrote ${output}`);
  console.log(`Installer URL: ${manifest.platforms[PLATFORM].url}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exit(1); }
}

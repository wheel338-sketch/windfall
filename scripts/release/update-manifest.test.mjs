import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  PLATFORM,
  main,
  projectFormatVersion,
  readPublicKey,
  releaseAssetUrl,
  updateManifest,
  verifySignature,
} from "./update-manifest.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const b64 = (text) => Buffer.from(text, "utf8").toString("base64");

/** A minisign key pair and signer in Tauri's base64 file form, made with Node alone. */
function minisignKey(id = Buffer.from("0123456789abcdef", "hex")) {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  const publicFile = `untrusted comment: minisign public key: ${id.toString("hex").toUpperCase()}\n${Buffer.concat([Buffer.from("Ed"), id, raw]).toString("base64")}\n`;
  return {
    publicKey: b64(publicFile),
    signFile(data, comment, prehashed = true) {
      const message = prehashed ? createHash("blake2b512").update(data).digest() : data;
      const signature = sign(null, message, privateKey);
      const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
      const algorithm = prehashed ? "ED" : "Ed";
      return b64(`untrusted comment: signature from tauri secret key\n${Buffer.concat([Buffer.from(algorithm), id, signature]).toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`);
    },
  };
}
const data = Buffer.from("Windfall installer fixture bytes");
const file = "Windfall_1.0.1_x64-setup.exe";
const comment = `timestamp:1760400000\tfile:${file}\tversion:1.0.1`;

test("accepts the updater's own signature form, prehashed or not", () => {
  const key = minisignKey();
  for (const prehashed of [true, false]) {
    const fields = verifySignature({ data, signature: key.signFile(data, comment, prehashed), publicKey: key.publicKey, version: "1.0.1", file });
    assert.equal(fields.version, "1.0.1");
    assert.equal(fields.file, file);
  }
});

test("refuses another key, changed bytes, a forged comment, another version or file", () => {
  const key = minisignKey();
  const signature = key.signFile(data, comment);
  const other = minisignKey(Buffer.from("fedcba9876543210", "hex"));
  const sameId = minisignKey();
  const changed = Buffer.from(data); changed[0] ^= 1;
  assert.throws(() => verifySignature({ data, signature, publicKey: other.publicKey, version: "1.0.1", file }), /different key/);
  assert.throws(() => verifySignature({ data, signature, publicKey: sameId.publicKey, version: "1.0.1", file }), /does not match/);
  assert.throws(() => verifySignature({ data: changed, signature, publicKey: key.publicKey, version: "1.0.1", file }), /does not match/);
  const text = Buffer.from(signature, "base64").toString("utf8").replace("version:1.0.1", "version:9.9.9");
  assert.throws(() => verifySignature({ data, signature: b64(text), publicKey: key.publicKey, version: "9.9.9", file }), /trusted comment is not signed/);
  assert.throws(() => verifySignature({ data, signature, publicKey: key.publicKey, version: "1.0.2", file }), /version 1.0.1, not 1.0.2/);
  const unbound = key.signFile(data, `timestamp:1760400000\tfile:${file}`);
  assert.throws(() => verifySignature({ data, signature: unbound, publicKey: key.publicKey, version: "1.0.1", file }), /version \(none\)/);
  assert.throws(() => verifySignature({ data, signature, publicKey: key.publicKey, version: "1.0.1", file: "Other_1.0.1_x64-setup.exe" }), /not Other/);
  assert.throws(() => verifySignature({ data, signature: b64("not minisign"), publicKey: key.publicKey, version: "1.0.1" }), /not a base64 minisign/);
});

test("reads the key the app embeds", () => {
  const config = JSON.parse(fs.readFileSync(path.join(here, "../../apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  const { id } = readPublicKey(config.plugins.updater.pubkey);
  assert.equal(id.length, 8);
  assert.deepEqual(config.plugins.updater.endpoints, [`https://github.com/wheel338-sketch/windfall/releases/latest/download/latest.json`]);
});

test("writes Tauri's static manifest with Windfall's channel and project format", () => {
  const manifest = updateManifest({ version: "1.0.1", notes: "Notes", pubDate: "2026-10-14T12:00:00.123Z", signature: "c2ln\n", url: releaseAssetUrl("1.0.1", file), format: 1 });
  assert.deepEqual(manifest, {
    version: "1.0.1",
    notes: "Notes",
    pub_date: "2026-10-14T12:00:00Z",
    platforms: { [PLATFORM]: { signature: "c2ln", url: "https://github.com/wheel338-sketch/windfall/releases/download/v1.0.1/Windfall_1.0.1_x64-setup.exe" } },
    windfall: { channel: "stable", projectFormatVersion: 1 },
  });
  assert.throws(() => updateManifest({ version: "1.0.2-test", notes: "", pubDate: "2026-10-14", signature: "s", url: "u", format: 1 }), /no preview channel/);
  assert.equal(projectFormatVersion("pub const FORMAT_VERSION: u32 = 3;"), 3);
  assert.equal(releaseAssetUrl("1.0.2", "Windfall Update Test_1.0.2_x64-setup.exe"), "https://github.com/wheel338-sketch/windfall/releases/download/v1.0.2/Windfall%20Update%20Test_1.0.2_x64-setup.exe");
});

test("the command verifies before it writes, and writes nothing for a bad signature", () => {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), "windfall-manifest-"));
  try {
    const key = minisignKey();
    const installer = path.join(folder, file);
    fs.writeFileSync(installer, data);
    fs.writeFileSync(`${installer}.sig`, key.signFile(data, comment));
    fs.writeFileSync(path.join(folder, "key.pub"), key.publicKey);
    fs.writeFileSync(path.join(folder, "notes.md"), "  Windfall 1.0.1\n\n- Updates.\n");
    const output = path.join(folder, "latest.json");
    main(["--installer", installer, "--version", "1.0.1", "--notes-file", path.join(folder, "notes.md"), "--public-key", path.join(folder, "key.pub"), "--pub-date", "2026-10-14T12:00:00Z", "--output", output]);
    const manifest = JSON.parse(fs.readFileSync(output, "utf8"));
    assert.equal(manifest.notes, "Windfall 1.0.1\n\n- Updates.");
    assert.equal(manifest.platforms[PLATFORM].signature, fs.readFileSync(`${installer}.sig`, "utf8").trim());
    assert.equal(manifest.windfall.projectFormatVersion, projectFormatVersion());
    fs.rmSync(output);
    fs.writeFileSync(installer, Buffer.concat([data, Buffer.from("tampered")]));
    const run = spawnSync(process.execPath, [path.join(here, "update-manifest.mjs"), "--installer", installer, "--version", "1.0.1", "--notes-file", path.join(folder, "notes.md"), "--public-key", path.join(folder, "key.pub"), "--output", output], { encoding: "utf8" });
    assert.equal(run.status, 1);
    assert.match(run.stderr, /signature does not match the file/);
    assert.equal(fs.existsSync(output), false);
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
});

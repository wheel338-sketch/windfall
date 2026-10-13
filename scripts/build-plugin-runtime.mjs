// Runs only during distributor builds, before Tauri packages resources.
// No engine DLL is resolved from a user's DAW or working directory.
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const target = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.WINDFALL_PLUGIN_TARGET
const universal = target === "universal-apple-darwin"
const targets = universal ? ["aarch64-apple-darwin", "x86_64-apple-darwin"] : [target]
for (const buildTarget of targets) {
const args = buildTarget ? ["--target", buildTarget] : []
for (const artifact of [["-p", "windfall-plugin", "--lib"], ["-p", "windfall-plugin-host", "--bin", "windfall-plugin-audio"]]) {
  const result = spawnSync("cargo", ["build", "--release", "--jobs", "1", "--target-dir", path.join(root, "target"), ...args, ...artifact], { cwd: root, stdio: "inherit", windowsHide: true })
  if (result.status !== 0) throw new Error("Windfall plugin runtime/helper build failed")
}
}
const nativeTriples = { "x64-win32": "x86_64-pc-windows-msvc", "x64-linux": "x86_64-unknown-linux-gnu", "arm64-darwin": "aarch64-apple-darwin", "x64-darwin": "x86_64-apple-darwin" }
const triple = target || nativeTriples[`${process.arch}-${process.platform}`]
if (!triple) throw new Error("Unsupported Windfall plugin runtime platform")
const windows = /windows|win32/.test(triple)
const mac = /darwin|macos/.test(triple)
const library = windows ? "windfall_plugin.dll" : mac ? "libwindfall_plugin.dylib" : "libwindfall_plugin.so"
const helper = `windfall-plugin-audio${windows ? ".exe" : ""}`
const output = path.join(root, "target", ...(target ? [target] : []), "release")
const destination = path.join(root, "target", "release", "plugin-runtime")
fs.mkdirSync(destination, { recursive: true })
for (const name of [library, helper]) {
  if (universal) {
    const result = spawnSync("lipo", ["-create", ...targets.map(t => path.join(root, "target", t, "release", name)), "-output", path.join(destination, name)], { cwd: root, stdio: "inherit", windowsHide: true })
    if (result.status !== 0) throw new Error("Universal Windfall runtime/helper assembly failed")
  } else fs.copyFileSync(path.join(output, name), path.join(destination, name))
  if (!windows) fs.chmodSync(path.join(destination, name), 0o755)
}
const version = /^version\s*=\s*"([^"]+)"/m.exec(fs.readFileSync(path.join(root, "Cargo.toml"), "utf8"))?.[1]
if (!version) throw new Error("Missing workspace runtime version")
fs.writeFileSync(path.join(destination, "distribution.json"), JSON.stringify({
  abi: 1, version, target: triple,
  files: [library, helper].map(name => ({ name, sha256: createHash("sha256").update(fs.readFileSync(path.join(destination, name))).digest("hex") })),
}, null, 2))

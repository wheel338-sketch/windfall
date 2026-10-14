# Release engineering: first R1 packet

This packet implements a candidate build and evidence pipeline. It does not publish a release or complete phase 7/R1. Existing CI, Cargo/package/Tauri versions, runtime, updater, CSP, credentials, resources and global roadmap/parity accounting are unchanged.

## Candidate contract

`.github/workflows/release-candidate.yml` accepts an explicit full lowercase source commit, a future semantic version already present in that commit, `prerelease` or `stable`, and a distribution mode. The workflow is written in the JSON subset of YAML 1.2 so its syntax and contract can be checked with Node built-ins. Syntax acceptance alone does not establish GitHub workflow validity. `scripts/release/check-workflow.mjs` rejects duplicate fields, unavailable expression contexts/functions in the supported workflow locations, unsupported action pins, unsafe shell expression interpolation and publication commands. Its expression checks are bounded project checks, not a replacement for GitHub's full parser/schema or server-side validation.

The workflow tools are checked out separately at the immutable workflow commit. The requested source is checked out at the requested commit. Preflight validates the workspace version, **every workspace Cargo package**, desktop package version, Tauri version, product name/identifier, clean source, resource contract and published identities. Inputs reach shell commands through quoted environment variables. Nothing changes source versions on the runner. Source commits need to contain the real factory crate and desktop build, and all existing integration/repair gates must pass before the parent authorizes a candidate run.

Only `unsigned-development` is implemented. `signed-release` fails closed, including when operator profile/verifier names are present: the native signing/verifier integration still requires a serialized implementation window. It cannot fall back to development mode. A stable channel input does not make an unsigned candidate a stable published distribution. There are no signing secrets wired into this workflow.

Native build rows:

| Runner             | Native target              | Bundle and exact filename              |
| ------------------ | -------------------------- | -------------------------------------- |
| `windows-2022`     | `x86_64-pc-windows-msvc`   | NSIS: `Windfall_VERSION_x64-setup.exe` |
| `macos-15` (ARM64) | `aarch64-apple-darwin`     | DMG: `Windfall_VERSION_aarch64.dmg`    |
| `ubuntu-22.04`     | `x86_64-unknown-linux-gnu` | Debian: `Windfall_VERSION_amd64.deb`   |

The collector also checks the actual native Node OS/architecture. Intel macOS, Windows ARM64, RPM and AppImage are not admitted yet. The inspected Tauri AppImage bundler can download `linuxdeploy-plugin-appimage` from a mutable `continuous` release and fall back to a different implementation. That requires a reviewed, pinned tool/input contract before admission. Ubuntu 22.04 provides the existing WebKitGTK 4.1 baseline; actual minimum OS/runtime compatibility still requires install tests.

Builds use frozen pnpm installation, `tauri build --ci --no-sign --target ... --bundles ... -- --locked`, the existing Linux prerequisites, and the existing Windows `scripts/msvc-env.sh`. Cargo jobs and Rust test threads are each 1. Generated bindings go to runner temp. The fresh native target is `source/target/release-r1-native`; build targets and previous bundles are **not** restored from cache. Only locked package sources/stores are cached. The factory verifier runs before bundling. No new npm/Rust application dependency is added.

Workflow artifacts are named `unsigned-development-VERSION-PLATFORM-ARCH-RUN-ATTEMPT`, retained for 14 days, uploaded only after verification, and never overwritten. Repository permission is `contents: read`; checkout credentials are not persisted. Read-only API tokens are available only to the two identity checks, not to the native build steps. Uploading a workflow artifact is not uploading a GitHub Release asset. Parent alone handles future publication after reviewing all gates. Dispatch requires the workflow to be available through the repository's Actions/default-branch workflow rules; this worker does not dispatch or push it.

## Immutable published alpha

`v0.1.0-alpha.1` is permanently reserved. Its annotated tag object is `9e7094f1ec1053cbae83d48067fc594fc593855a`; its dereferenced commit is `157f96fd068b46adab046a6880ce27ea4b49aea1`.

| Existing asset                         |   Bytes | SHA256                                                             |
| -------------------------------------- | ------: | ------------------------------------------------------------------ |
| `Windfall_0.1.0-alpha.1_x64-setup.exe` | 9070359 | `51b1ab3a6d1fa2ad0f6bfb78c9bacf9ecc1bb73a4415dd1c6dfc5db7286a81a3` |
| `SHA256SUMS.txt`                       |     103 | `520617b06f4fb04b1bbd75b51cc23fa23143a7b0df3902b0ecdceb4ad75a78d6` |

The tools reject that version, its source commit, and any semantic version at or below it. Remote preflight checks the tag object/dereference, release flags and asset sizes/digests when GitHub supplies digests. It rejects existing version tags and existing published **or draft** releases, and repeats the check after building. Missing API evidence or pagination failure is an error. These read-only checks do not download assets and are not a fresh byte verification of the historical installer. A race after the check remains a publication-time gate; the candidate pipeline cannot reserve a release identity by mutating GitHub.

The present source version remains `0.1.0-alpha.1`, so it intentionally cannot pass candidate preflight. The parent must approve and commit a fresh matching version in all reserved metadata/lock entries before running the pipeline. A fixture using an authored future version is not evidence that the current app is released.

## Manifest, checksums and source evidence

Each platform produces a separate candidate directory containing:

- `manifest.json`: schema version 1, expected source/version/channel, platform/architecture/triple, workflow commit/run/runner image and actual Git/Node/pnpm/Rust/Cargo/Tauri tool versions, exact artifact paths/sizes/SHA256, signing/notarization states and unresolved publication gates.
- `SHA256SUMS.txt`: sorted checksums for every inventoried payload **and the manifest**; the checksum file does not hash itself.
- `installers/`: only the expected native bundle for that row.
- `source/Windfall-COMMIT.tar`: `git archive` of the exact expected commit with a fixed prefix and tar umask. It includes checked-in build/install sources and lockfiles. It does not substitute a branch archive URL for corresponding source.
- `source/inventory.json`: complete tracked source inventory, Git blobs/modes/tree, exact checkout byte hashes, and source capture digest.
- `resources.json`: exact source resource mapping, every factory file's size/hash and packaged destination, generator reference and provenance limitations.
- `dependencies.json`: a machine-readable **Windfall lock-backed SBOM**, all Cargo/npm lock entries (including build and optional platform packages), pinned registry checksums/integrities, actual installed declared licenses, provenance/notice gaps and the resolved Cargo graph, including activated features and dependency kinds/platform predicates. It is not labelled SPDX or CycloneDX conformance.
- `notices/inventory.json` and `notices/`: source notices and available dependency license/notice texts. Workspace packages without a separate license file use the repository license text. This is evidence collection, not a license-compatibility decision.

The generator checks the supplied capture against current source before and after collection. Source reads are checked against their immutable Git blob, including files hidden with `assume-unchanged`; Git-confirmed LF-to-CRLF checkout conversion is allowed and the actual checkout bytes are still recorded. Source symlinks/submodules and archive `export-ignore`/`export-subst` attributes are rejected; Git's global archive attributes are disabled. The verifier independently checks the externally supplied commit/version/channel, recreates the source archive, repeats workspace/resource/lock/notices checks, hashes real artifacts, and rejects omissions, tampering, duplicate logical artifacts, case collisions, path traversal, absolute paths, symlink/junction escapes, malformed/duplicate JSON fields, unknown states and contradictory signing claims. The manifest cannot remove publication blockers or claim compliance.

Paths are portable forward-slash relative names. Sorting uses explicit codepoint order, not locale order. Windows device names, alternate streams and trailing dots/spaces are rejected. Candidate files are nonempty regular files. Inventory JSON and checksums have fixed ordering/newlines and contain no generated wall-clock timestamp. Repeated inventories with identical inputs/provenance produce identical bytes; per-OS checkout line endings, different run identities, compiler/tool/runner versions, installer timestamps, signatures/notarization receipts and bundler downloads can change outputs. This packet does **not** promise byte-reproducible signed installers. JSON object order is canonical in produced evidence; deliberately reordered metadata is rejected rather than silently normalized.

Outputs are staged in an owned sibling directory. The final directory is reserved with an exclusive mkdir; each file is installed by an atomic no-clobber hard link, with the manifest linked last as the completion marker. Existing final directories/files and a competing destination are preserved. A failed final installation can leave an incomplete directory without a completion marker; rerun into a new destination after inspecting it. Staging cleanup checks its resolved parent, prefix and ownership token and never deletes the whole source/workspace. The destination filesystem must support local hard links. Capture output uses the same exclusive temporary-file/link approach.

Hashes establish integrity, not publisher authenticity. Treat workflow provenance and the original candidate manifest as trusted review inputs; there is no signed attestation yet. In particular, native executable identity, embedded resources, loaded libraries, signer chains and installability are not established merely by recognizing a filename. They are explicit blockers. Security state `signature: not-verified` is intentional: `--no-sign` disables requested distribution signing, but the collector does not pretend to verify every binary or classify compiler/platform ad-hoc signatures. macOS notarization is `not-requested`; other rows use `not-applicable`.

## License, resource and future integration gates

Cargo evidence comes from `cargo metadata --locked` and Cargo.lock. npm evidence comes from actual installed package declarations/notice bytes and the v9 pnpm lockfile. The collector independently discovers the local pnpm virtual store and flat/nested `node_modules`; package links must resolve inside the desktop store. Multiple peer contexts must agree on declaration and notice bytes. This no longer relies on a caller's summarized license groups. The lock reader admits pinned registry identities/checksums only and fails on unsupported source forms/lock shapes. Installed dependency paths must stay in the source/declared Cargo registry/npm stores. Optional packages absent on the current platform remain `not-installed-on-this-platform`; missing declarations or notice texts remain `missing-license-or-notice`. Malformed or inconsistent metadata fails generation. Missing provenance appears as an explicit manifest blocker. All declared licenses still require review; no license expression is rewritten into a supposed GPL-equivalent license.

Independent verification reconstructs the entire expected dependency inventory from fresh locked Cargo metadata, installed npm declarations and original notice bytes, and compares it with the submitted inventory. GPL-to-MIT relabelling, omitted/added Cargo edges, changed features/kinds/platform predicates, omitted installed packages and altered notice inventories fail even when the candidate's artifact hashes/checksums are recomputed. The exported offline-input collector also compares supplied Cargo metadata with a fresh authoritative read; caller metadata cannot authenticate itself. Cargo's graph is the default workspace metadata graph, including its platform predicates, not an extracted binary dependency graph. npm is a complete lock-entry inventory; it does not claim a reconstructed npm edge graph. Native contents/system-library and complete corresponding-source/license review remain pending. Verification needs the corresponding source and the installed package stores from collection, not just the uploaded manifest.

The exact-commit source tar is the project's corresponding-source starting point. Complete distribution obligations still require review of dependency corresponding source, vendored/native/bundler tools and platform libraries, any build scripts and special license/notice requirements. The Cargo/npm inventory is not a complete binary system-library SBOM. The candidate always has `publication.eligible: false`. A successful candidate gate must never be described as GPL compliance or production approval.

The accepted source resource seam is exactly `../../../content/factory/` to `factory/`. Its 33 current WAVs plus README/license must match the actual Rust factory manifest, and the workflow's Rust factory verifier checks generated bytes. The CC0 dedication belongs to those sounds; the generator remains GPL-3.0-or-later. Source resource evidence is not an extracted-installer inspection. Platform files, platform config overrides, extra resource maps, additional desktop binary targets and `bundle.externalBin` require an accepted contract before this collector admits them. Admission rejects every JSON/JSON5/TOML platform filename supported by the pinned CLI (`tauri.PLATFORM.conf.json`, `tauri.PLATFORM.conf.json5`, `Tauri.PLATFORM.toml`, for Windows/macOS/Linux/Android/iOS), as well as alternate base JSON5/TOML files. Directory entries are checked even when ignored by Git; no alternate merge can silently introduce helpers/models. The exact upstream [filename and merge contract](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri-utils/src/config/parse.rs) was checked for this repair.

N4 must first supply an accepted helper binary name/target inventory, parent-owned runtime discovery choice, ABI/auth/lifecycle contract and verified packaged native fixtures. Only then can a serialized window add the agreed `bundle.externalBin` or resource map and corresponding verifier. M1 must first supply an accepted model/resource manifest with exact content digests, origin, license text, redistribution/corresponding-source obligations and runtime discovery policy. No helper, weights or inference claim is added by this packet. Parent serializes both seams; worker foundation code does not establish working inference or shipping resources.

## Operator signing, notarization and updater requirements

The `WINDFALL_SIGNING_PROFILE` and `WINDFALL_SIGNING_VERIFIER` names identify future reviewed operator configuration; they are **not** Tauri environment variables and do not unlock the current signed gate. Do not put secrets in dispatch inputs, source config, logs or a candidate directory.

Windows requires an operator-owned certificate/provider and approved timestamp service. Tauri's inspected Windows config has `certificateThumbprint`, `digestAlgorithm`, `timestampUrl`, `tsp`, or an approved `signCommand` using `%1` as its binary placeholder. The certificate must be provisioned to the native runner/provider without exposing its key. A later implementation must verify actual app/helper/installer Authenticode chains, signer identity, digest and timestamp before declaring verified signing. No certificate thumbprint, timestamp URL or provider credentials are invented here.

macOS outside-App-Store distribution requires an operator-owned Developer ID Application identity, hardened runtime/approved entitlements and notarization. Official Tauri inputs include `APPLE_SIGNING_IDENTITY`, `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`; notarization can use `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`, or the approved Apple-ID path `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`. Import keys only in an isolated runner keychain with reviewed cleanup. Ad-hoc identity `-` is not authenticated distribution signing. A later verifier must run actual codesign/Gatekeeper checks plus stapled-ticket validation for the shipped app/DMG, with receipts bound to its hashes. The current workflow neither imports certificates nor requests notarization.

Linux requires an agreed signing/distribution policy and verifier for the exact Debian package/manifest plus repository trust. No Linux signing key/account is configured here.

Auto-update is configured from 1.0.1; see "Publishing an update" below. Tauri updater signatures are distinct from OS code signing: the installer stays Authenticode-unsigned.

## Publishing an update (Windows, from 1.0.1)

Windfall updates itself from GitHub Releases. The configuration lives in one place, the `plugins.updater` block of `apps/desktop/src-tauri/tauri.conf.json`:

- `pubkey`: the updater public key (`windfall-updater.key.pub`, base64 as Tauri stores it). Every release must be signed with the matching private key, which is kept outside the repository (`C:/Users/ewhee/.windfall-release/windfall-updater.key` on the release machine).
- `endpoints[0]`: the stable channel, `https://github.com/wheel338-sketch/windfall/releases/latest/download/latest.json`. GitHub serves the newest non-draft, non-prerelease release's asset there. There is no preview channel.
- `requireSignedVersion: true`: a signature must name the version it was made for, so an old signed installer cannot be replayed under a higher version number.

The app checks at start-up at most once a day (Settings → Updates turns it off) and from Help → Check for updates. It accepts only a higher, non-prerelease version whose manifest declares `windfall.channel: "stable"` and the app's project format, downloads with a size cap, verifies the minisign signature, then installs through the shutdown path (unsaved-work prompt, audio and plugins stopped, rollback if cancelled or refused) and relaunches.

Release steps:

1. Bump the version (workspace `Cargo.toml`, `Cargo.lock`, `apps/desktop/package.json`, `tauri.conf.json`, `crates/windfall-plugin/src/clap.rs`) and commit.
2. Build with the key. From Git Bash after `source scripts/msvc-env.sh`, prebuild the plugin runtime (`cargo build --release -p windfall-plugin --lib` and `cargo build --release -p windfall-plugin-host --bin windfall-plugin-audio`), then:

   ```bash
   export TAURI_SIGNING_PRIVATE_KEY="$(cat C:/Users/ewhee/.windfall-release/windfall-updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   pnpm --dir apps/desktop tauri build --ci --bundles nsis
   ```

   Do not pass `--no-sign`: it skips the updater signature too. Without the key the build fails ("A public key has been found, but no private key"); unsigned development bundles still build with `--no-sign`. The build writes `target/release/bundle/nsis/Windfall_VERSION_x64-setup.exe` and `…-setup.exe.sig`.
3. Write the manifest. The script verifies the signature against the embedded key (key id, Ed25519 signature, signed version and file name) before it writes anything:

   ```bash
   node scripts/release/update-manifest.mjs      --installer target/release/bundle/nsis/Windfall_VERSION_x64-setup.exe      --notes-file docs/RELEASE-NOTES-VERSION.md
   ```

   It writes `latest.json` beside the installer: `version`, `notes`, `pub_date`, `platforms.windows-x86_64.{signature,url}` with the URL `https://github.com/wheel338-sketch/windfall/releases/download/vVERSION/Windfall_VERSION_x64-setup.exe`, and `windfall.{channel,projectFormatVersion}`.
4. Create the GitHub release `vVERSION` as a draft, upload the installer, its `.sig` and `latest.json` (in that order, asset names exactly as built), then publish it as the latest release, not a prerelease. `releases/latest/download/latest.json` moves to the new release only when it is published, so it never points at an installer that is not there yet.
5. Check: `curl -sL https://github.com/wheel338-sketch/windfall/releases/latest/download/latest.json` returns the new version, and an installed previous version finds it from Help → Check for updates.

To test against another host, build with an override instead of editing the committed file: `pnpm --dir apps/desktop tauri build --ci --bundles nsis --config override.json`, where `override.json` sets `plugins.updater.endpoints` (HTTPS, or literal loopback HTTP such as `http://127.0.0.1:18765/latest.json`). `scripts/auto-update-window-smoke.mjs` runs the end-to-end check this way with a separate product identity; see `docs/qa/2026-10-14-auto-update.md`.

Windfall 1.0.0 shipped without an updater configuration, so its users must install 1.0.1 by hand once. Updates work from 1.0.1 onwards. A lost private key cannot be replaced without another manual install, because installed versions trust only the embedded public key.

## Local commands and handoff

Use Node 26.4.0 (executed locally) or the pinned workflow toolchain. These commands write no release metadata and use no signing material:

```powershell
node --test --test-concurrency=1 scripts/release/release.test.mjs scripts/release/action-provenance.test.mjs
node scripts/release/check-workflow.mjs
node scripts/release/action-provenance.mjs check
```

For an approved clean future-version checkout, first capture outside the checkout, build native bundles with the exact workflow flags, then collect and verify. The CLI requires all identity fields on every command; it never derives the expected source identity from the manifest under review:

```text
node scripts/release/cli.mjs preflight --source SOURCE --commit COMMIT --version VERSION --channel prerelease --mode unsigned-development --github
node scripts/release/cli.mjs capture --source SOURCE --commit COMMIT --version VERSION --channel prerelease --mode unsigned-development --output CAPTURE.json
node scripts/release/cli.mjs candidate --source SOURCE --commit COMMIT --version VERSION --channel prerelease --mode unsigned-development --capture CAPTURE.json --bundle-root BUNDLE_DIR --platform windows --architecture x64 --output NEW_CANDIDATE_DIR
node scripts/release/cli.mjs verify --source SOURCE --commit COMMIT --version VERSION --channel prerelease --mode unsigned-development --candidate NEW_CANDIDATE_DIR
```

`--github` uses an already configured read-only `GH_TOKEN` and is mandatory in the workflow identity steps. It is optional for offline local fixtures/collection. Local Windows compilation must use Git Bash after sourcing `scripts/msvc-env.sh`, an isolated `target/release-r1-native`, jobs 1 and a task-specific binding-output directory. Run one Cargo build at a time. The fixture suite invokes Cargo metadata only; it does not compile Rust, generate bindings/WASM or install tools. Authored fixture bundles are real byte fixtures, not installable app claims.

Requested next parent integration window, after this foundation is accepted:

1. After parent acceptance and repaired combined-source gates, parent can select `0.1.0-alpha.2` and supply the exact combined source commit, then serialize matching workspace/every Cargo member, desktop package, Tauri and lock-entry versions. This is a concrete proposal, not a selected version or an open metadata window; no source metadata is changed here. Preserve the existing identifier/product identity and immutable alpha.
2. Optionally add a narrow existing-CI job/steps using `node --test --test-concurrency=1 scripts/release/release.test.mjs` and `node scripts/release/check-workflow.mjs` with Node 26.4.0; existing CI is untouched here.
3. Serialize `apps/desktop/src-tauri/tauri.conf.json` (or a reviewed dedicated release override) for `bundle.windows.allowDowngrades: false`, approved Windows signing fields, macOS hardened-runtime/identity/entitlements and native verification plumbing. Do not enable a signed-release mode until the verifier binds verified signer/notarization receipts to actual artifact hashes.
4. Admit N4/M1 resources only after their accepted contracts above, then serialize updater dependencies/capabilities/runtime/config after the operator supplies approved key/host references and recovery tests. Request only that bounded window; do not reopen timeline/DSP/native runtime ownership.

Executed locally on Windows at checkpoint `6719106`: all 38 Node fixtures passed (no skips), including real temporary artifact bytes, generator/verifier, CLI success/failure exits, source/archive/metadata tampering, CRLF, junction escapes, no-clobber contention and missing/contradictory provenance. The unchanged Tauri config and an in-memory future-version copy passed the installed 2.12.1 schema with existing Ajv; non-Unicode regex mode was required for the schema's escaped filename pattern and uint8/uint32 formats were explicitly registered. Existing Prettier's YAML parser accepted the workflow, all 14 shell command blocks passed Git Bash syntax checking, and the workflow contract check passed. The actual lock readers parsed 538 Cargo and 589 npm entries. CLI preflight rejected current alpha with exit 1 as required. Formatting and whitespace checks passed. No native app compilation or environment tool installation was needed for these checks.

Still unexecuted: GitHub dispatch; real new-version Windows/macOS/Linux bundle runs; authenticated signing/notarization; extracted package/native dependency inventories; fresh-user install/uninstall/upgrade with project/settings preservation; and update-host/runtime recovery tests. Full R1 also retains crash consent/recovery, tested CSP, localization/tutorials, native multi-window polish and macOS/Linux renderer/accessibility verification. This packet closes none of those phase/parity rows.

## Verified upstream contracts and pins

Inspected local Tauri CLI/schema version 2.12.1 and upstream Tauri CLI tag commit `30da1fd6e17de6107ecc850c95dfb16b5729f2dd` for native filename/build contracts. Current action commits below were resolved through read-only upstream GitHub refs on 2026-10-08; all four upstream repository licenses report MIT, and their runtime is Node 24. Their committed lockfiles/distribution bundles pin their transitives; there is no floating action or third-party Rust-toolchain/pnpm-setup action added here. Registry metadata/integrities were checked for the 174 unique actual runtime transitives across these action locks, resolving npm aliases and legacy SHA1 integrities correctly. 173 have declared licenses (MIT, ISC, Apache-2.0, BSD, 0BSD, BlueOak-1.0.0 or the exact combined expressions). The follow-up source evidence below records the unresolved `buffers@0.1.1` grant separately from the action maintainer's MIT review comment. The `build-tool-license-provenance` publication blocker remains; no GPL equivalence or closed tool-license review is inferred.

Existing pnpm 11.1.2 was verified against registry metadata: MIT, Node >=22.13, no separately declared runtime dependencies; distribution integrity `sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==`. Its bundled dependency/native tool obligations remain subject to the distribution review above. Rust 1.99.0 and Node 26.4.0 match the installed local tools; official Node distribution checksums exist for that exact version. Native NSIS/DMG bundler download/source closures and runner-provided tools still require the same build-tool review before production distribution; this packet introduces no custom remote tool or `curl|sh` installer.

| Action                           | Verified commit                            |
| -------------------------------- | ------------------------------------------ |
| `actions/checkout` v7.0.1        | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `actions/setup-node` v7.1.0      | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
| `actions/cache` v6.1.0           | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` |
| `actions/upload-artifact` v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |

## Action source/notice evidence and applicability

`scripts/release/action-provenance.json` is a separate CI toolchain inventory. Its collector reads the four repositories at the workflow's exact commits, rejects truncated Git trees, and verifies each selected raw file's size, Git blob identity and SHA256. The 231 checked files cover root licenses, action definitions, package/lock metadata, every `src/` file, committed JavaScript distributions/package descriptors and available `.licenses/` records. Runtime locks contain 24/59/51/148 entries for checkout/setup-node/cache/upload-artifact respectively, or 174 unique actual name/version identities after aliases are resolved. The audit preserves their lock license declarations and integrities; it does not treat a root MIT license as a grant for every transitive. This is a source-byte audit, not a rebuild proving that TypeScript compiles reproducibly to the committed JavaScript.

Pinned license/notice locations:

| Action          | Root grant                                                                                                      | Additional checked notice sources                                                                                                                                                                              |
| --------------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| checkout        | [MIT LICENSE](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/LICENSE)        | [24 `.licenses/npm` records](https://github.com/actions/checkout/tree/3d3c42e5aac5ba805825da76410c181273ba90b1/.licenses/npm)                                                                                  |
| setup-node      | [MIT LICENSE](https://github.com/actions/setup-node/blob/949feb2413d6458794dcd2491c4babbbce0c15c1/LICENSE)      | [59 `.licenses/npm` records](https://github.com/actions/setup-node/tree/949feb2413d6458794dcd2491c4babbbce0c15c1/.licenses/npm)                                                                                |
| cache           | [MIT LICENSE](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/LICENSE)           | [44 records plus aggregate NOTICE](https://github.com/actions/cache/tree/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/.licenses)                                                                                   |
| upload-artifact | [MIT LICENSE](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/LICENSE) | [six direct-dependency records](https://github.com/actions/upload-artifact/tree/cf430e030ddbb5b0abf93d22962f4752f3646cd9/.licenses/npm); no aggregate NOTICE or separate dist license file in this pinned tree |

When conveying action code, its MIT copyright/permission notice must accompany the covered code. Preserve original third-party license/copyright/NOTICE material and review the actual transitive obligations too; these record counts are not proof of complete coverage. Existing inline distribution notices are retained upstream; this worker neither strips them nor vendors those executable bundles. The committed Windfall inventory contains URLs/hashes and declarations, not copies of the action implementations or third-party license texts.

The upload entrypoint is [the pinned `dist/upload/index.js`](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/dist/upload/index.js), 4,452,820 bytes, SHA256 `c0e58b240360573e6d6c82ea1bb22deff4f85883e00211a52bd27822c3a22617`. Its [source upload helper](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/src/shared/upload-artifact.ts) delegates file upload to `@actions/artifact`. The lock chain is `@actions/artifact@6.3.1` → `unzip-stream@0.3.4` → `binary@0.3.0` → `buffers@0.1.1`. This proves inclusion, not runtime reachability of every bundled module. The [upstream review configuration](https://github.com/actions/upload-artifact/blob/cf430e030ddbb5b0abf93d22962f4752f3646cd9/.licensed.yml) comments `buffers` as MIT; that is an action-maintainer declaration. It does not supply a recovered copyright-holder license/notice for the exact historical package.

The official [npm version metadata](https://registry.npmjs.org/buffers/0.1.1) and its package declaration have no license field. The six-file tarball has no license/notice file. Tarball SHA256 is `f8de49c60e467005182d9687b5d87775bbcec6385ac63220b7741327441deb6d`; its bytes match the lock's SHA512 integrity and npm's SHA1. Its complete `package/index.js` is 7,057 bytes, SHA256 `97801e296ba5f3c32242f647e1daac5917e300367fed7935ab1e62f40adb1b8a`, and occurs byte-for-byte at offset 606,901 in both committed upload/merge bundles. The referenced `substack/node-buffers` repository returned HTTP 404 during collection. No missing grant is invented, and no unknown-license executable code is copied into this repository. Resolving this particular closure requires an authoritative grant/notice tied to these bytes, or a separately reviewed action replacement/removal with an established transitive closure. No upstream contact or replacement is performed here.

These actions execute outside the source/build payload on the Actions runner. The candidate collector stages only its explicit installers/source/evidence/notices; it does not stage the fetched action repositories. Action references and this hash inventory in Windfall's source archive do not convey those implementations. Accordingly `buffers` is a **CI toolchain provenance concern**, not an asserted Windfall runtime dependency or an automatic product-license equivalence decision. Native installer-content review remains pending, so this statement cannot authenticate actual installers. The existing broad `build-tool-license-provenance` publication blocker is retained for the unresolved action closure and native/bundler/runner tool review; development-candidate collection is not license approval or production publication.

Reproduce with built-in Node reads only:

```text
node scripts/release/action-provenance.mjs check
node scripts/release/action-provenance.mjs reproduce
node scripts/release/action-provenance.mjs capture --output NEW_EVIDENCE_FILE
```

`check` validates local evidence shape/policy/pins without network and explicitly reports unresolved closure. `reproduce` refetches official pinned repository/package bytes and compares the entire deterministic inventory with the checked record; changed source, missing evidence or registry/repository status changes fail. `capture` creates an exclusive atomic output and refuses existing files. These commands read no secret/environment credentials, install nothing and never evaluate downloaded code. Inspection of the historical tarball is in memory, with bounded decompression, path/header/checksum/link/truncation gates. No GitHub write or workflow execution is involved.

The independent review of `671910694f4be9397e5bb63f9da730e920143f5d` remains attributable to that immutable checkpoint. This follow-up repairs its P1 alternate-config admission finding and P2 SBOM source-binding finding, and includes the two small nonblocking CLI/tag-loop deduplications. Focused pre-repair tests reproduced the JSON5 admission and both independent declaration/edge failures. The original fixture suite remains part of follow-up validation; no native/signing/installation evidence is inferred from these repairs.

Follow-up Windows validation: the combined 56-case Node loop passed 55, with one existing npm fixture's boundary expectation invalidated by moving its package into the independently discovered source store. The boundary setup was corrected; its targeted rerun passed 1/1 and additionally checked agreeing/conflicting peer-context declarations and notices. All 56 distinct fixtures therefore have passing executed results, including the original 38; the full loop was not repeated. No test was skipped. Every P1/P2 regression passed in the full loop, including correct-hash/checksum forged inventories and actual CLI failures. The action evidence's official-source `reproduce` check passed, all 10 Node modules passed syntax checks, and the unchanged workflow contract/format/whitespace checks passed. Cargo metadata was read for authored local fixtures only; no Cargo build, app install, tool install, native packaging, GitHub dispatch or publication occurred.

## GitHub workflow-context repair after checkpoint 477

The authorized parent source push `290f0313709d94581240eb38d5b3b70356f7b631` produced [run 37756196886](https://github.com/erivgout/windfall/actions/runs/37756196886): event `push`, failure, no jobs. Check suite `102286500929` has zero check runs, no annotations and is not rerequestable. The API exposes no diagnostic reason. This was a workflow-validation failure, not a dispatched candidate, build or upload. Its workflow Git blob `cb5814ac2e9803fe5e2ffb35c7b03093d9b3a686` matches immutable checkpoint `477209ec1d3c5f2d7efe5e7147b5551a4bfbe0c1`.

GitHub's published compiled `@actions/workflow-parser@0.3.61`, source commit `b71ac284fc6a7382c22ecbf3e0009a6ae216823b`, reproduces three `Unrecognized named-value: 'runner'` errors on that exact workflow, lines 139–141. It accepts the JSON syntax. A minimal YAML workflow reproduces the same error with `runner.temp` in job `env`; moving only that value to step `env` removes it. GitHub's [context-availability table](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#context-availability) and the [pinned official schema](https://github.com/actions/languageservices/blob/b71ac284fc6a7382c22ecbf3e0009a6ae216823b/workflow-parser/src/workflow-v1.0.json) exclude `runner` from job `env` and admit it in step `env`. This establishes a concrete semantic defect; the server's undisclosed reason and acceptance of the repair still require parent follow-up.

The repair moves `TS_RS_EXPORT_DIR`, `CANDIDATE_OUTPUT` and `SOURCE_CAPTURE` unchanged to a first candidate step. That step exports the values for later steps using the documented [`GITHUB_ENV` contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-an-environment-variable). It adds no trigger or action, and preserves manual-only dispatch, read-only permissions, pinned checkouts/actions, locked installs, unsigned development mode, verification before no-clobber upload and every source/publication/license/resource gate. The new `workflow.test.mjs` fixtures run alongside the existing release fixtures in preflight.

The one-off official parser harness was isolated in owned temporary storage. Registry SHA512 integrities were checked before execution; actual license files were inspected. No npm installation/lifecycle scripts, project dependency or lockfile changes were used. The exact runtime closure was:

| Package                           | License                   | Registry tarball SHA256                                            |
| --------------------------------- | ------------------------- | ------------------------------------------------------------------ |
| `@actions/workflow-parser@0.3.61` | MIT, Copyright GitHub     | `fc0c4af6cc1f6c8a6f68e47e2093fc826a922c7244a0ffd2f8030a9303a59877` |
| `@actions/expressions@0.3.61`     | MIT, Copyright GitHub     | `a09137eed52067f4369a5fdff1e2beb3663a1e9388a4709a414ada2dd44c82cd` |
| `yaml@2.7.0`                      | ISC, Copyright Eemeli Aro | `8be868c685838f23ab425b046733b69def708ff3bd1716f806ad1c0eada31650` |
| `cronstrue@2.21.0`                | MIT, Copyright Brady Holt | `c62b2b601253a1a26c4411ddfb2841423d22c7df6245ee55254f69f15fa0f7c9` |

The two Actions packages share that pinned source commit. YAML/cronstrue versions match its upstream lockfile and have no declared runtime dependencies; expression parsing has no declared runtime dependencies. A local Node ESM loader supplied missing extension resolution and JSON import attributes without modifying upstream code. To reproduce the official comparison, provision these exact packages outside the repository, verify the listed tarball hashes/licenses, call `parseWorkflow({name, content}, new NoOperationTraceWriter())`, then `await convertWorkflowTemplate(result.context, result.value)` and inspect `result.context.errors.getErrors()`. Checkpoint 477 returns the three errors; the repaired full workflow returns none. This is one official parser implementation, not a GitHub service attestation.

The authored scope regression loop first executed red (2 passed, 11 failed against the old checker), then green after the repair. The final 20-fixture loop passed with no skips. It exercises real validator CLI failures, unavailable bracket-form contexts, global/job scope restrictions, step-only functions, unterminated expressions, valid step contexts and quoted literals. Further fixtures execute the actual path-export command with spaces/quotes/metacharacters and retain trigger/permission/pin/signing/verification/overwrite gates. All 15 workflow shell blocks passed Git Bash syntax checking; both changed Node modules passed syntax checks, and the existing action-provenance shape/policy check still reports unresolved license closure. Formatting/whitespace checks passed. Preserve this workflow's strict JSON syntax by selecting `--parser json` when formatting its `.yml` file. Run `node --test --test-concurrency=1 scripts/release/workflow.test.mjs` and `node scripts/release/check-workflow.mjs` for the bounded dependency-free regression checks.

Parent must check the next authorized source push's GitHub workflow registration/validation without dispatching a candidate. The existing 56-fixture result is separate from this narrow repair; it is not rerun or re-attributed here. Actual native bundles, installation, signing/notarization, updates and unresolved `buffers@0.1.1` provenance remain gated.

Primary references: [Tauri distribution/build contract](https://v2.tauri.app/distribute/), [Windows signing](https://v2.tauri.app/distribute/sign/windows/), [macOS signing/notarization](https://v2.tauri.app/distribute/sign/macos/), [Debian baseline](https://v2.tauri.app/distribute/debian/), [updater](https://v2.tauri.app/plugin/updater/), [GitHub workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax), [runner architectures for private repositories](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [secure use of Actions](https://docs.github.com/en/actions/reference/security/secure-use), [artifact upload contract at the pinned action](https://github.com/actions/upload-artifact/tree/cf430e030ddbb5b0abf93d22962f4752f3646cd9), [Node 26.4.0 distribution checksums](https://nodejs.org/dist/v26.4.0/SHASUMS256.txt). These are configuration references, not evidence that Windfall passed external release gates.

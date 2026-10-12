# Phone remote, optional backup and Windfall loops

These are desktop capabilities. The browser preview reports that native access
is required. None of the features selects a cloud account or starts a listener
or backup on installation.

## Phone and tablet remote

In Settings, enter the desktop's specific local IPv4 address and port, then
enable Phone remote. Open the displayed HTTP address on a phone on the same
network and enter the desktop's pairing code. A loopback address works only on
the desktop itself. Wildcard, public and IPv6 binds are refused. Firewall and
network access are controlled by the user and operating system.

The page supports Play, Stop, mixer volume, a channel selector, a keyboard and
16 pads. It reads transport and meters from the desktop's existing realtime
carrier. Faders use the same validated `SetTrackParam` command as the mixer and
create normal undo history. Transport calls share the UI's native transport
methods. Live notes use the existing audition-note API. Remote edits, transport
and note-on are refused while recording; note-off can release held notes.
Commands carry the native project generation and cannot act on a replacement
project under reused IDs. Disconnection releases this connection's held notes.

This is plain HTTP/WebSocket on a trusted local network. Tokens and music
control are not encrypted in transit. Do not forward the port to the Internet.
TLS, discovery, multiple paired devices and Internet access are not implemented.
Disable revokes the pairing authority and all connections. Enable again to get
a fresh code. The listener closes asynchronously; immediately rebinding the
same port may briefly fail until its worker exits.

### Protocol contract

- `GET /` serves a self-contained page. `POST /pair` accepts only `{ "code":
  "123456" }` and returns `{ "token": "<64 lowercase hex characters>" }`.
- The six-digit code is random, single-use, expires after five minutes and
  permits at most ten attempts per enable. The token contains 256 random bits,
  remains in page memory, and expires after eight hours.
- `GET /ws` upgrades using RFC 6455 version 13. Each masked, complete text frame
  is `{ "token": "...", "sequence": 1, "generation": 7, "action":
  { "name": "feed" } }`. Sequence numbers strictly increase for that token
  across connections. Unknown envelope/action fields are refused.
- Named actions: `feed`, `play`, `stop`, `fader` (`track`, finite `value` in
  0..2), `noteOn` (`channel`, MIDI `key` in 0..127, finite `velocity` in 0..1),
  and `noteOff` (`channel`, `key`). Feed is read-only and supplies the current
  generation. Replies acknowledge sequence and contain an error or fresh state.
- Requests require the exact bound IP/port in Host. A supplied Origin must
  match that HTTP origin. There is no CORS permission or token in a query string.
- Bounds: four workers, twenty new connections/second, forty messages/second
  per token, 8 KiB HTTP headers, 4 KiB request bodies/frames and 64 KiB response
  frames. Reads have a two-second elapsed deadline plus a two-second socket
  timeout; writes have a two-second socket timeout. No chunked HTTP, binary,
  fragmented, compressed or extended WebSocket frames are accepted.
- Held notes are bounded to 32 per connection and leased for five seconds.
  Requests expire old leases; a silent connection times out and releases all
  notes. The page sends a read-only feed request every 100 ms while idle.

## Optional folder backup

The Browser's Cloud backup tab lets the user choose an existing dedicated
folder for one project history. It can be a local folder or a folder already
managed by the user's synced-drive client. The user chooses that client and its
account. Windfall writes files to the chosen folder; the drive client controls
whether and when those files leave the computer.

Provider-neutral folder backup is the agreed backup product. It requires no
Windfall account, cloud provider SDK or direct cloud adapter. The adapter
contract below documents an optional future extension.

Back up now captures a portable archive using the existing archive export path,
including referenced audio and native plugin state. See
[Portable projects](PORTABLE-PROJECTS.md). Export never marks a project clean.
After saving a project once, the user may enable backup after Save. The option
is stored per saved project path and covers ordinary Save and Save new version;
numbered saves carry the chosen configuration to their new path. Save as to an
unrelated name does not carry it automatically. Backup failure reports an error
without failing the completed Save. Capture retains the original native project
generation and refuses a replacement document. Edits during Save or export may
be included in the later captured backup; it is a portable snapshot, rather
than an exact copy of the earlier on-disk Save bytes.

History lists timestamps, sizes, IDs and SHA-256 checksums. Refresh history
explicitly acknowledges the current head for the next backup. A competing
writer causes a conflict instead of overwriting history; review the versions
before retrying. Retention keeps 1..1000 versions. Cancellation is checked
between archive/copy chunks and before publication. A committed backup remains
committed even if Cancel arrives afterward.

Restore chooses a **new** ZIP destination, verifies length and SHA-256, and
publishes without overwriting an existing file. The normal Open project flow
then handles unsaved changes and recording exclusion. A successful restored ZIP
remains available if the user declines to replace the current project.

### Folder storage and recovery

`windfall-history.json` stores oldest-to-newest `BackupVersion` rows with `id`,
`parent`, `checksum`, `bytes` and `createdMs`. Objects are immutable `<id>.zip`
files. IDs and checksums are 64 lowercase hexadecimal characters. Limits are
2 GiB per archive, 2 MiB history and 4096 parsed history entries. Copies use
64 KiB chunks. A create-new `.windfall-backup-lock` serializes cooperating
writers and restores on this filesystem. Publication uses a staged, synced
object and an atomic history replacement; retention deletes expired objects
only after the new history commits. Cleanup errors can leave unreferenced
objects; they do not invalidate published history.

After a crash, a lock file may remain. Stop all owners and inspect the history
and folder before manually removing that lock. Windfall never guesses that a
lock is stale. Filesystem durability after sudden power loss needs platform
validation. Locks do not provide atomic remote transactions between disconnected
replicas of a synced folder. Resolve the client's conflicting copies explicitly;
do not treat a synced folder as a distributed database. Checksums detect
accidental corruption; they are not signatures against an attacker able to edit
both the history and archive.

### Real provider adapter contract

`apps/desktop/src-tauri/src/backup.rs` defines `BackupProvider: Send + Sync`:

```rust
fn versions(&self) -> Result<Vec<BackupVersion>, String>;
fn publish(&self, archive: &Path, expected: Option<&str>, keep: usize,
           cancel: &AtomicBool) -> Result<BackupVersion, String>;
fn restore(&self, id: &str, target: &Path,
           cancel: &AtomicBool) -> Result<(), String>;
```

An adapter must preserve these semantics:

1. Return a bounded, ordered, validated history. `expected` is the head the user
   actually reviewed; `None` means the remote history must be empty.
2. Stream the portable archive with cancellation and size limits, calculate its
   SHA-256, and upload a new immutable object. Never replace an existing object
   or make an incomplete upload visible as a version.
3. Atomically compare-and-publish the new history against the expected remote
   head, using the provider's conditional write/ETag/generation transaction.
   Local lock files or a blind overwrite of a remote JSON file are insufficient.
   Losing the comparison returns a conflict and leaves the prior head intact.
4. Check cancellation before the head transaction. That transaction is the
   commit boundary. Cancellation afterward must not remove the committed
   object. Clean up only request-owned unreferenced uploads.
5. Apply retention only after commit, never deleting a retained/shared object.
   Report or log cleanup failures without claiming the backup transaction failed.
6. Restore into a request-owned staged file, verify bytes and checksum, sync,
   check cancellation, and publish to a new destination without overwrite.
   Restore never bypasses the desktop's normal project-open guards.
7. Keep network, auth, compression, hashing and filesystem work on blocking
   control workers. Credentials must stay out of project files, history,
   frontend persistence, URLs and logs. Retry operations must be idempotent;
   bounded deadlines and resumable-transfer policy belong to the adapter.

To enable a real provider, add explicit provider/account configuration and
route the session job to its chosen `BackupProvider`. The current session job
selects the fully implemented folder provider. No SDK, provider or credential
flow has been selected. For a future direct adapter, the user must decide provider and account, remote
folder/bucket and per-project naming, authentication and secure credential
storage, transfer/quota policy, retention, conflict handling, and whether to add
client-side encryption/key recovery. A real adapter needs live credentials,
provider CAS/concurrency, cancellation and restore acceptance tests in addition
to the folder harness.

## Windfall's own starter loops

Eight original two-bar stereo arrangements cover House, Techno, Hip hop,
Breakbeat, Ambient, Funk, Drum and bass and Synth pop, at source tempos of
75..174 BPM and several minor keys. The code in
`crates/windfall-factory/src/loops.rs` combines Windfall's synthesized drums and
oscillators. It imports no recordings, proprietary presets or third-party
audio. Generated audio is dedicated to CC0 under the factory licence; the
generator code retains Windfall's GPL licence.

The Loops tab filters genre, source key and tempo. Audition uses source tempo
and key. Insert or drag to a playlist row resynthesizes the loop at the current
project tempo (40..300 BPM) and the chosen minor pitch class. Keeping source key
is the default. Tempo resynthesis does not change note pitch. Insertion creates
a normal sample, audio clip and required playlist/mixer tracks as one undo step.
Empty rows up to the drop destination are included in that step. Generation,
revision and recording checks reject work prepared for a changed document.

The clip spans exactly two 4/4 bars in project ticks. It is fitted at insertion;
changing the project's tempo later does not rerender this audio automatically.
There are no major-mode arrangements in this first library. The selected key is
explicit because Windfall has no inferred project-wide musical key.

`Loops/manifest.json` records every shipped asset's ID, relative path, genre,
tempo, pitch class (C=0), mode, bars and `loops-v1` provenance. Runtime-derived
WAVs live in the settings recordings folder under `FactoryLoops`, with
content-addressed names and `.wav.json` sidecars containing source identity,
source/fitted tempo and key, bars, SHA-256 and provenance. Existing bytes and
sidecars must match before reuse. Cache sources are retained for undo/redo and
saved projects, and portable archives include them as ordinary audio.

The integrator regenerates the checked-in factory folder, including eight WAVs,
its manifest and README, after merging:

```sh
source scripts/msvc-env.sh # Windows Git Bash
cargo run -p windfall-factory --release -- generate content/factory
cargo run -p windfall-factory --release -- verify content/factory
```

The implementation worker has not run these commands or any tests. Listening,
cross-platform byte identity, real phones/firewalls and actual synced-drive or
cloud credentials remain external acceptance gates. The exact written tests and
integrator sequence are in the
[implementation hand-off](qa/2026-10-11-remote-cloud-content.md).

# AI agents (MCP)

Windfall can let an AI agent on your computer work on the open song with
you. It speaks the [Model Context Protocol](https://modelcontextprotocol.io),
so Claude Desktop, Claude Code and other MCP clients can connect to it.

An agent connected to Windfall can:

- **read** the project: tempo, meter, the key its notes suggest, channels and
  their instruments, patterns and their notes, playlist tracks and clips,
  mixer tracks with their effects and routing, and the settings of any
  instrument or effect, with the ids it needs to refer to them;
- **propose edits**: notes, step-sequencer steps, patterns, channels,
  effects, settings, routing, sends, clips, tempo, meter, names, colours and
  automation. Every edit is a proposal you see in Windfall and apply or
  reject. Applied, it is one undo step named after the agent;
- **listen**: render a region offline and get its loudness, peaks, clipping,
  level over time, energy per octave, tempo and key, and the WAV file.

It cannot open, save or delete files, reach the network, or change anything
without your approval (unless you allow it, below).

## Turning it on

Open **Settings** and find **AI agents (MCP)**. It is off until you press
**Turn on the MCP server**. Windfall then listens on `127.0.0.1` only, on the
port shown (47133 unless you choose another before turning it on), and stays
on across restarts until you turn it off.

The page shows a **bearer token**, made once for this installation. A client
that connects over HTTP must send it; the stdio bridge reads it by itself.
**New token** replaces it and disconnects every agent that used the old one.

The page also shows the snippets below with this computer's paths filled in,
each with a **Copy** button.

## Connecting a client

Windfall must be running with the server on. Both ways reach the same
server.

### Claude Desktop (stdio bridge)

Claude Desktop starts a command and talks to it over standard input and
output. The Windfall executable does that when it is started with
`--mcp-stdio`: it opens no window, finds the running Windfall and relays the
messages to it.

The Windows installer puts Windfall in your own user folder, so the
executable is

```text
C:\Users\<you>\AppData\Local\Windfall\windfall-desktop.exe
```

(`%LOCALAPPDATA%\Windfall\windfall-desktop.exe`). The settings page shows
the exact path on your computer, already filled into the snippets below.

Add this to `claude_desktop_config.json` (on Windows
`%APPDATA%\Claude\claude_desktop_config.json`, on macOS
`~/Library/Application Support/Claude/claude_desktop_config.json`), with your
own user name, and restart Claude Desktop:

```json
{
  "mcpServers": {
    "windfall": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\Windfall\\windfall-desktop.exe",
      "args": ["--mcp-stdio"]
    }
  }
}
```

On macOS the command is the executable inside the app bundle, for example
`/Applications/Windfall.app/Contents/MacOS/windfall-desktop`. Release 1.0.1
ships for Windows only, and this path has not been checked.

The bridge reads the port and token from `mcp.json` in Windfall's settings
folder (`%APPDATA%\org.windfall.Windfall` on Windows). If you start Windfall
with another settings folder (`WINDFALL_CONFIG_DIR`), give the bridge the
same one: add `"--config-dir", "<folder>"` after `"--mcp-stdio"`, or set the
same `WINDFALL_CONFIG_DIR` in the server's `env`.

If Windfall is not running or the server is off, the bridge exits at once
with code 1 and one line on its standard error saying why, which the client
shows in its log.

### Claude Code

Through the stdio bridge:

```sh
claude mcp add --transport stdio windfall -- "C:\Users\<you>\AppData\Local\Windfall\windfall-desktop.exe" --mcp-stdio
```

Or over HTTP, with the token from the settings page:

```sh
claude mcp add --transport http windfall http://127.0.0.1:47133/mcp --header "Authorization: Bearer <token>"
```

Add `--scope user` to either to have Windfall in every project.

### Other clients

Any MCP client that supports the Streamable HTTP transport can use
`http://127.0.0.1:<port>/mcp` with the header `Authorization: Bearer <token>`.
Windfall serves protocol revision `2026-07-28` and the earlier
`initialize`-based revisions `2025-11-25`, `2025-06-18` and `2025-03-26`.
Answers are plain JSON, without server-sent events.

### What was checked

Both transports were checked against the running app with the official
[MCP Inspector](https://github.com/modelcontextprotocol/inspector) 2.11.0
command-line client, in both protocol eras (`--protocol-era legacy`,
`modern` and `auto`). These are the exact forms used, with the default port:

```sh
# Streamable HTTP, with the token from the settings page
npx @modelcontextprotocol/inspector@2.11.0 --cli http://127.0.0.1:47133/mcp --transport http --header "Authorization: Bearer <token>" --method tools/list

# stdio, through the bridge
npx @modelcontextprotocol/inspector@2.11.0 --cli --method tools/list --transport stdio -- "C:\Users\<you>\AppData\Local\Windfall\windfall-desktop.exe" --mcp-stdio
```

`scripts/mcp-client-smoke.mjs` repeats the whole exchange (reading, edits
approved in the panel, undo, stale and rejected proposals, a render, the
refusals), and `docs/qa/2026-10-14-mcp.md` records the results. Claude
Desktop and Claude Code themselves were not run against it; the settings
above follow their documented formats. An agent appears in Windfall under the
name its client gives (the Inspector calls itself `inspector-cli`).

## Approving what an agent proposes

When an agent proposes an edit, a panel appears at the bottom left of the
main window. You can keep working while it waits. Each proposal shows the
agent, the name the undo step will have, the agent's explanation, how many
notes, clips, channels and so on it adds or removes, and under **Show the
commands** the exact commands it holds.

- **Apply** applies it as one undo step. Edit > Undo takes it back.
- **Reject** refuses it. The agent is told.
- A proposal goes **stale** when the project changes after it was made, by
  you or by another proposal. It cannot be applied then; the agent is told
  and can make a new one from the current project.
- Nothing applies while you are recording.

Under the proposals, **Let <agent> apply its proposals without asking** lets
that agent's proposals apply as soon as it makes them, still as one undo step
each and still shown in the panel. This lasts until Windfall quits or the
server is turned off. Agents are told apart by the name their client gives,
so treat this as a convenience for clients you connected yourself.

When an agent renders, the panel says so, with **Stop the render**.

## Tools

Read:

| Tool | What it does |
|---|---|
| `get_project_summary` | Tempo, meter, the key the notes suggest, channels with their instruments, patterns, playlist tracks, mixer tracks in use with effects and routing, automations, samples, with ids. Also the resource `windfall://project/summary`. |
| `list_notes` | The notes of a pattern (optionally of one channel): id, pitch, start, length, velocity, pan. Paged. |
| `list_clips` | The clips over a stretch of the song, optionally of one playlist track. Paged. |
| `get_mixer_track` | One mixer track: level, pan, its effect chain, sends, sidechains, output and inputs. |
| `get_parameters` | The settings of an instrument, an effect or a mixer track's own processing: name, id, range, unit, default and current value. Paged and searchable. |
| `get_selection` | The pattern being edited, the transport mode, and the channel, mixer track and view you selected last. |
| `list_instruments_and_effects` | Every built-in instrument and effect kind. |

Edit (each files a proposal):

| Tool | What it proposes |
|---|---|
| `create_pattern` | A new pattern, with a name, colour and length. |
| `add_notes` | Notes for one channel in one pattern, optionally replacing the lane or what they overlap. |
| `remove_notes` | Removing notes by id, or by range and pitch. |
| `update_notes` | Changing notes by id: pitch or transpose, start, length, velocity, pan. |
| `set_steps` | Step-sequencer steps as text such as `x...x...x...x...`. |
| `add_channel` | A channel playing a built-in instrument. |
| `add_effect`, `remove_effect`, `move_effect` | The effect chain of a mixer track. |
| `set_parameter` | Settings by name, checked against their ranges; volume, pan, mute, effect mix. |
| `route_channel`, `route_track` | Where a channel or a mixer track plays into. |
| `set_send` | A send between mixer tracks, or removing it. |
| `place_clips`, `move_clips`, `remove_clips` | Pattern and automation clips on the playlist. |
| `set_tempo_meter` | The tempo and the meter. |
| `rename_and_color` | Names and colours of channels, patterns, mixer tracks and playlist tracks. |
| `write_automation` | An automation curve for a volume, pan, send, effect or instrument setting, or the tempo, put on the playlist. |
| `apply_raw_commands` | Windfall's own commands (docs/SCRIPTING.md), for anything else a proposal may hold. |

Proposals:

| Tool | What it does |
|---|---|
| `get_proposal` | How a proposal stands; can wait up to 60 seconds for your decision. |
| `apply_proposal` | Applies the agent's own pending proposal, only when you allowed it to apply without asking. |
| `withdraw_proposal` | Takes back the agent's own pending proposal. |

Listen:

| Tool | What it does |
|---|---|
| `render_and_measure` | Renders the song, a stretch of it, or a pattern, optionally with one mixer track soloed, offline and at most 120 seconds, and returns integrated loudness (LUFS), loudness range, true and sample peak, clipping, RMS over time, stereo correlation, energy per octave band, detected tempo and key, and the WAV file's path. |

Positions are `bar:beat:tick`, with bars and beats counted from 1 and 960
ticks to a quarter note; lengths are note values such as `1/8` or `1/4.`;
pitches are note names such as `C4` (middle C) or MIDI numbers.

## Safety

- **Off by default.** Nothing listens until you turn it on.
- **This computer only.** The server binds `127.0.0.1`. Requests must name
  that address as their host and carry no web page's origin, which stops a
  web page from reaching it through DNS rebinding.
- **A token.** Every HTTP request must carry the installation's bearer token.
  It is kept in `mcp.json` in Windfall's per-user settings folder (on Unix
  readable by you only; on Windows with that folder's own rights), and shown
  only on the settings page.
- **Bounded.** Requests are limited in size and time, connections in number
  and rate, and renders to 120 seconds.
- **Approved edits only.** An agent's edits are the same checked, undoable
  command batches as the built-in assistant's, limited to editing music: no
  files, no project replacement, no plugin state. Each applies only when you
  apply it (or allowed the agent to), only if the project has not changed
  since it was made, never while recording, and always as one undo step.
- **No files.** No tool reads, writes or deletes files. Renders are written
  only to the `Agent renders` folder in Windfall's recordings folder, which
  keeps the 20 newest.

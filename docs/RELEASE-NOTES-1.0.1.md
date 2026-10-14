Windfall 1.0.1 brings two things: Windfall now updates itself, and AI agents such as Claude can work on your song with you through the new MCP server, which is off until you turn it on and changes nothing without your approval.

## Automatic updates

Windfall looks for a new release once a day at start-up, or when you choose Help → Check for updates, and offers to install it now or later.

Installing asks about unsaved work, then restarts Windfall in the new version. Every update is signed, and Windfall verifies the download before anything runs. Automatic checks can be turned off in Settings → Updates.

If you have Windfall 1.0.0, install 1.0.1 by hand from this release once; later versions arrive by themselves.

## AI agents (MCP server)

Windfall speaks the Model Context Protocol, so an AI agent on your computer (Claude Desktop, Claude Code or another MCP client) can connect to the running app and:

- read the open project: tempo, meter, channels, patterns and notes, playlist, mixer, effects and their settings;
- propose edits: notes, steps, patterns, channels, effects, settings, routing, clips, tempo, names, colours and automation;
- render a stretch of the song offline and measure it: loudness, peaks, clipping, level over time, energy per octave, tempo and key.

What keeps it safe:

- It is **off by default**. Turn it on in Settings → AI agents (MCP). The page shows the port, the access token and the exact settings to paste into Claude Desktop or Claude Code.
- It is **local only**: it listens on 127.0.0.1, needs the token, and refuses requests from web pages.
- **Every edit needs your approval.** An agent's edit appears in a panel at the bottom left of the window, where you can apply or reject it. Applied, it is one undo step named after the agent. You can let an agent you connected yourself apply its edits without asking, until Windfall quits.
- Agents cannot open, save or delete files.

See docs/MCP.md for how to connect a client and what every tool does.

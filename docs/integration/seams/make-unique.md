# Make unique

`playlist.makeUnique` ("Make unique") appears immediately after Duplicate in
the playlist clip menu and beside Bounce selected clips in the panel menu.
It has no shortcut and is enabled in the playlist when at least one selected
clip is a pattern or audio clip.

The pure `makeUniqueClipIds` helper keeps selected pattern and audio clip ids
in the given order. An automation clip is skipped. An empty selection or one
containing only automation clips returns no ids and dispatches nothing.

The action dispatches one batch labeled "Make unique", with one existing
`{ type: "makeUnique", clip }` command per eligible clip. The command copies
the pattern or the sample reference so later edits to that new source do not
change the other clips. Other clips that still share the old source stay linked.
The batch is one undo step.

Focused coverage: `apps/desktop/src/features/playlist/make-unique.test.ts`.
Run from `apps/desktop`:

```sh
pnpm test -- src/features/playlist/make-unique.test.ts
```

## Automation clips — 2026-10-10

Make unique now acts on every kind of clip. For an automation clip the
command copies the automation, with its target, points and colour and a
numbered name, places the copy right after its source, and redirects only
the chosen clip. The clip keeps its window on the curve. Other clips of the
original automation stay linked to it. `makeUniqueClipIds` returns every
selected clip in order.

Coverage: `crates/windfall-project/tests/playlist_layouts.rs` and
`crates/windfall-engine/tests/engine/playlist_layouts.rs` (run);
`features/playlist/make-unique.qa.test.ts` (written, not yet run).

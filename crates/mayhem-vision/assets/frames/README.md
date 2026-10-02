# Augment card frames

`silver.png`, `gold.png`, `prismatic.png`: the three augment card frames, 310×512 RGBA with a
transparent centre. Supplied by the project owner on 2026-09-29 and baked into the app
(`include_bytes!` in `src/frames.rs`).

Only their **outline** is used for matching (edge strength over a band around the opaque pixels).
Rarity does not come from these PNGs at all: the game draws the frames far brighter than the assets
and makes the Prismatic one pulse, so rarity is read from the *broad colour* on screen — the share
of the frame's lit pixels that are warm yellow (Gold) or violet-to-magenta (Prismatic), with Silver
as the near-neutral remainder. The opaque mask here is what says which pixels those are.

# The "hide augments" button

`hide-augments.png`: the in-game button that shows and hides the augment cards, 207×64 RGBA. It is
**a crop of a real screenshot**, not a redrawing: cut from
`tests/fixtures/screens/augments_1920x1080_gold.jpg` by `tests/augment_button_template.rs`, at the
teal plate measured by `tests/augment_button_measure.rs` grown by 8 px on every side so the gold
outer border — the strongest edge on the button — is included.

The button is what tells us the augment screen is open. It earns that job by being the one
piece of the augment screen that never changes: it is present exactly while augments can be
selected, it does not swap its glyph when toggled, and, unlike the card frames, it has no rarity
variants and no glow. The card frames only answer the narrower question of whether the cards are
shown or hidden right now.

Matching is greyscale NCC (`imageops::locate_ncc_in`), which is invariant to brightness and
contrast, so a hover highlight or a different background behind the button does not move the score.

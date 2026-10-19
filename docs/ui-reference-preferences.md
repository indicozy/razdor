# UI reference preferences

## Three rectangular window sections

User reference, 2026-10-18: the battle exit window consists of three adjoining rectangular
blocks: the title at the top, the large content area, and the button footer. Each block has
a continuous solid silver boundary of the same thickness. Shared horizontal boundaries
are drawn once, at the same thickness as the outside edges. Avoid inset frames, gaps,
double lines, and dark secondary strokes between these three blocks.

The user may request this treatment for other windows. Apply it to windows they identify;
it is not a request to change all windows automatically. The warning panel inside the
content area has its own separate decorative frame.

Implementation: `chrome::three_block_frame`, drawn after section backgrounds and artwork.

## Frame material and hierarchy (reference set, 2026-10-18)

Use the original `SteelLine` first row for thin silver outlines. Structural borders use
a continuous neutral silver band two native pixels wide, without texture restart seams
or extra dark strokes. Inset illustrations, lists and text panels use one native pixel,
with at least one screen pixel of clearance from structural borders. Shared section edges
are drawn once. Draw the building/army structural grid after tab content, at fixed positions.

Modal windows with a button footer have three sections: title, content, buttons. Content
can contain independently outlined pictures and parchment panels. Army/building windows
instead follow their existing left column, upper right content, hint strip and lower right
content; do not force a footer layout onto them. Preserve their left parchment and artwork.

Scenario artwork uses `MapImageFrame` with an 11-native-pixel inset opening and fine silver
outlines at the outer edge and picture opening. Keep its decorative border separate from
the description's parchment ornaments. Background illustrations in exit windows remain
visible through the darkened warning panel, which has corner decorations only.

In battle, extend the left parchment and its separator to the window's bottom behind the
autobattle buttons; reserve the original text area above those buttons. The exit artwork's
thin frame follows its actual bounds, and the window height follows the artwork plus footer.

## Reference details and quick actions (2026-10-19)

The two exit windows share the battle-exit footer dimensions. Keep its first three
buttons against the left and Cancel against the right, vertically centred in the footer.
Draw held items after structural frames. Army hint strips contribute artwork only inside
building/army windows: their enclosing structural grid owns the border exactly once.

Quest and rumour lists on light parchment use the original mirrored side shape in gold
(subtract RGB 32,40,192), separately from brown-paper decoration. Diary contents and entry
panels keep their side ornaments and fine silver outlines but omit corner flourishes.
Equipment cells are 55 original pixels scaled by 15/16, touch vertically in each pair,
and retain their dark backing and silver outline beneath equipped items. The bottom
command ribbon uses a one-native-pixel top edge.

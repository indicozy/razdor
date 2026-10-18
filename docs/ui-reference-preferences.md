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

Use the original `SteelLine` first row for silver outlines: its changing neutral metallic
tones are visible along every edge. Do not replace it with a flat blue-grey colour or an
extra dark inner stroke. Structural borders use two native pixels; inset illustrations,
lists and text panels use one. Shared section edges are drawn once.

Modal windows with a button footer have three sections: title, content, buttons. Content
can contain independently outlined pictures and parchment panels. Army/building windows
instead follow their existing left column, upper right content, hint strip and lower right
content; do not force a footer layout onto them. Preserve their left parchment and artwork.

Scenario artwork uses `MapImageFrame` with an 11-native-pixel inset opening and fine silver
outlines at the outer edge and picture opening. Keep its decorative border separate from
the description's parchment ornaments. Background illustrations in exit windows remain
visible through the darkened warning panel, which has corner decorations only.

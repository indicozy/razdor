# Studio UX: icons and UI font

Ticket: indicozy/razdor#38. Date: 2026-11-04. Branch: research/studio-ux-icons-font (based on origin/v2-packages).
Source tags like [S12] point to the Sources list at the end. Local measurements are marked "measured" and were made in a scratch directory that is deleted afterwards.

## Recommendation

- **Icon set: Phosphor, Regular weight.** Every action on the owner's list has a Phosphor name that exists in the pinned package (two are substitutes: `layers` to `stack`, `unlock` to `lock-simple-open`). Licence MIT [S19][S23]. Lucide is a close second (ISC, 2313 icons) but has no egui 0.31 binding (see Drawing method).
- **Drawing method: the `egui-phosphor` crate, version `0.9.0`, feature `regular` (default).** It is the last release that targets egui 0.31 [S2][S3]. Licence MIT OR Apache-2.0. It adds one TTF of 488,636 bytes to the binary [S3].
- **UI font: Noto Sans Regular, the family already in the repo.** It is OFL-1.1 with no Reserved Font Name in its licence [S32][S34]. The shipped subset does not contain the 66 Russian letters (U+0401, U+0410-044F, U+0451), so regenerate it with U+0400-045F added: measured 234,776 bytes, against 234,808 now [measured]. PT Sans is first in the game's fallback chain and already draws the 66 Russian letters (measured), so Russian output does not change; the 4 codepoints of U+0400-045F that PT Sans lacks would now fall back to Noto [local src/ui/script_font.rs].
- **Second choice for the font: Inter 4.1 Regular**, subset to Latin and Cyrillic with hinting removed: 70,780 bytes [measured]. Its website claims a Reserved Font Name, which its LICENSE.txt does not declare, so that must be settled first [S26][S27].
- **Versions to pin:** `egui-macroquad = "0.17.3"` (egui 0.31.1), `egui_dock = "0.16"` (egui ^0.31, MIT), `egui-phosphor = "0.9.0"` (egui 0.31).
- **Binary-size note:** the studio build already carries egui's default fonts (Ubuntu-Light, Hack, Noto Emoji, emoji-icon-font, 1,407,752 bytes in total) [S10][S11]. Dropping them is the largest size lever, but Ubuntu-Light is under the Ubuntu Font Licence, which the FSF list does not cover (see Open points).

## Icon sets

### Coverage (names checked in the pinned npm packages: lucide-static 1.68.0, @phosphor-icons/core 2.1.1)

Every name below returned HTTP 200 from the pinned package [S17][S22]. The Phosphor names are also constants of `egui-phosphor` 0.9.0 (`src/variants/regular.rs`) [S3].

| Action | Lucide | Phosphor | Note |
|---|---|---|---|
| brush | `brush` | `paint-brush` | |
| eraser | `eraser` | `eraser` | |
| paint bucket (fill) | `paint-bucket` | `paint-bucket` | |
| eyedropper | `pipette` | `eyedropper` | Lucide's name is `pipette` |
| rectangle marquee | `square-dashed` | `selection` | Lucide also has `square-dashed-mouse-pointer` |
| lasso | `lasso` | `lasso` | Lucide also has `lasso-select` |
| magic wand | `wand-sparkles` | `magic-wand` | Lucide also has `wand` |
| layers | `layers` | `stack` | Phosphor substitute |
| eye | `eye` | `eye` | |
| eye off | `eye-off` | `eye-slash` | Lucide also has `eye-closed` |
| lock | `lock` | `lock-simple` | |
| unlock | `unlock` | `lock-simple-open` | Both sets also have `lock-open` |
| stamp | `stamp` | `stamp` | |
| move | `move` | `arrows-out-cardinal` | |
| zoom in / out | `zoom-in` / `zoom-out` | `magnifying-glass-plus` / `magnifying-glass-minus` | |
| hand (pan) | `hand` | `hand` | Phosphor also has `hand-grabbing` |
| play | `play` | `play` | |
| sword (battle) | `sword` | `sword` | |
| dice | `dice-5` | `dice-five` | Lucide also has `dices` |
| map | `map` | `map-trifold` | |
| story / flow graph | `workflow` | `flow-arrow` | |
| variables | `variable` | `brackets-curly` | Lucide `braces`; Phosphor `function` |
| units | `users` | `users` | Lucide `shield`; Phosphor `shield-star` |
| items | `package` | `package` | Both sets also have `backpack` |
| scores | `trophy` | `trophy` | Both sets also have `chart-bar` |
| terminal | `terminal` | `terminal` | Phosphor also has `terminal-window` |

No action is missing from either set. The two Phosphor substitutes are the only places where the obvious name was not used.

### Licence, weights, size, count

| | Lucide | Phosphor |
|---|---|---|
| Licence | ISC, with a separate MIT notice for the Feather-derived icons [S14][S18] | MIT, "Phosphor Icons" [S19][S23] |
| Package checked | lucide-static 1.68.0 (latest on npm) [S15] | @phosphor-icons/core 2.1.1 (latest on npm) [S20] |
| Icon count | 2313 SVG files in `icons/` (2333 names in the font's codepoint map) [S16] | 1512 per weight [S21] |
| Weights | one stroke style; stroke width is set per use | Thin, Light, Regular, Bold, Fill, Duotone (core package) [S21]. `egui-phosphor` 0.9.0 ships Thin, Light, Regular, Bold and Fill TTFs, but no Duotone [S3] |
| SVG sizes (bytes, sampled) | brush 474, eye 434, lock 384, paint-bucket 705, terminal 340 [S17] | paint-brush 568, eye 701, lock-simple 275, eyedropper 796, terminal 241 [S22] |
| Icon font | `font/lucide.ttf`, 980,804 bytes (full set) [S16] | `Phosphor.ttf` 488,636 bytes (Regular, inside the crate) [S3] |

Phosphor's font has 1531 constants in `egui-phosphor` 0.9.0 against 1512 files in core 2.1.1 [S3][S21]. The difference is not explained by the sources I read; the crate says it bundles "the latest" Phosphor and does not name the version [S3].

### Tabler (not clearly better)

Tabler has 5203 outline and 1054 filled icons in @tabler/icons 3.50.0, MIT [S24][S25]. It has no egui 0.31 binding: `egui-tabler` does not exist on crates.io [S39]. Its names were not checked. It is not clearly better for this list, because the extra icons do not cover any action that Phosphor misses, and it would need hand-written egui glue.

## Drawing method

| Option | Crate and version | egui requirement | Licence | Binary cost | Crispness at 16 and 24 px | Verdict |
|---|---|---|---|---|---|---|
| A. Icon font, crate | `egui-phosphor` 0.9.0 | `egui` ^0.31 [S2][S3] | MIT OR Apache-2.0; font MIT [S3][S23] | Regular TTF 488,636 bytes, included with `include_bytes` [S3]. A subset of our 30 icons is 9,680 bytes [measured], if we later write our own build step | Same path as text, see below | **Recommended** |
| B. Icon font, Lucide (own registration) | none; `lucide.ttf` from lucide-static 1.68.0 | any | ISC + MIT [S14][S18] | 980,804 bytes full; 13,512 bytes for our 32 icons [measured] | Same as A | Viable, but we would write the registration ourselves and the font is 2x larger |
| C. Lucide crate `egui-lucide` | 0.2.0 (egui ^0.37) or 0.1.0 (egui ^0.36) [S4] | 0.36/0.37, not 0.31 | MIT AND ISC [S4] | n/a | n/a | **Rejected**: no version for egui 0.31 |
| D. SVG rasterised at runtime, egui_extras | `egui_extras` 0.31.1, feature `svg` | `egui` ^0.31.1 [S6] | MIT OR Apache-2.0 [S6] | pulls `resvg` 0.37.0 (MPL-2.0) [S6][S7] and 83 crates in its subtree [measured] | Unhinted raster at the requested pixel size | Works, but heavy and MPL-2.0 |
| E. SVG rasterised with resvg directly | `resvg` 0.45.1 (Apache-2.0 OR MIT, Rust 1.67.1) [S7] | none (we upload RGBA to egui) | Apache-2.0 OR MIT [S7] | 73 crates in its subtree with default features; tiny-skia 0.11.4, usvg 0.45.1 [S8][measured] | Unhinted raster, same as D | Only if icons must be full colour |
| F. Build-time raster | resvg as a build-dependency only | none at runtime | as E | the output is PNG or alpha bytes; size not measured | Pixel-exact at the chosen size | Possible later, not needed now |

Other egui 0.31 icon crates exist but are outside the owner's set: `egui-remixicon` 0.31.1 has `egui` as an optional dependency [S5]. I did not evaluate its icon licence.

### Crispness (reasoned, not measured)

Font icons and text go through the same epaint path. epaint rasterises each glyph outline with `ab_glyph` at the requested scale, with no hinting, and writes the coverage into the glyph atlas. Font sizes and vertical offsets are rounded to the pixel grid [S12]. An SVG texture is rasterised once by resvg at the requested size and then drawn as an image with egui's texture filter (I did not check which filter is the default). Neither path is hinted. If the texture rectangle sits on whole pixels and its size equals the raster size, the two should look alike at 16 and 24 px. The SVG stroke geometry differs from a font glyph that was built from the same outlines, so small differences are expected. I did not render either path, so this is an estimate. The owner should look at a screenshot before deciding on pixel-level details.

### Binary size (reasoned from file sizes)

- Option A adds 488,636 bytes of TTF to the executable. Using `include_bytes`, the data is stored uncompressed [general Rust behaviour, not measured].
- Option D or E adds the resvg subtree: 49 (resvg 0.37 with egui_extras) to 73 (resvg 0.45 with default features) crates [measured]. Their code size was not measured, so I cannot give a byte figure. The option is not free.
- If we also want Lucide, the same icons cost roughly twice as much as Phosphor in font bytes [S3][S16].

## UI font

| Font | Russian letters (66) | Cyrillic U+0400-052F covered | Licence | File size (bytes) | Hinting in file | Verdict |
|---|---|---|---|---|---|---|
| Noto Sans Regular, full (v2.015) | 66 of 66 | 304 of 304 [measured] | OFL-1.1; no Reserved Font Name declared [S32][S34] | 431,364 unhinted; 621,572 hinted [S34] | None in the unhinted file [measured] | **Recommended** (see below) |
| Noto Sans Regular, subset in repo (shipped now) | 0 of 66 [measured] | 208 of 304 (all of U+0460-052F, none of U+0400-045F) [measured] | OFL-1.1 [local data/fonts/Noto-OFL.txt] | 234,808 | None [measured] | Cannot draw Russian UI text |
| Noto Sans, subset to the current set plus U+0400-045F (measured) | 66 of 66 | 304 of 304 | OFL-1.1 | 234,776 | none (`--no-hinting`) | The file to ship: no net size change |
| Inter 4.1 Regular (static, from release zip) | 66 of 66 [measured] | 249 of 304 [measured] | OFL-1.1; website claims an RFN, LICENSE.txt declares none [S26][S27] | 411,640 hinted (has fpgm, prep, cvt tables); 270,072 full without hinting [measured] | Hinted in the release file [measured] | Second choice |
| Inter 4.1 Regular, subset Latin and Cyrillic, no hinting | 66 of 66 | n/a | OFL-1.1 (modified version) | 70,780 [measured] | removed | Larger than the Noto subset |
| Inter 4.1 Variable | 66 of 66 | 249 of 304 | OFL-1.1 | 879,708; 221,196 subset [S29][measured] | None (no hint tables) [measured] | Only if ab_glyph supports variations (open) |
| Ubuntu-Light (egui default, already in every egui build) | 66 of 66 [measured] | 214 of 304 | Ubuntu Font Licence 1.0 [S11] | 361,676 | Hint tables present, not applied by epaint [measured][S12] | Already in the binary; licence unresolved for GPL |

Notes:

- **Hinting does not matter in egui.** epaint uses `ab_glyph` outlines and does not run any hinting code [S12][S13]. The hint tables in Inter's static TTF and in Ubuntu-Light are dead bytes for egui. That is why the no-hinting subsets are the right files to ship. The claim that `ab_glyph` applies no hinting is my reading of its description and of epaint's source, not of ab_glyph's own code.
- **Cyrillic coverage.** Noto Sans is the only candidate whose full Cyrillic block is covered. Inter covers all Russian letters and ё, but not all 304 codepoints. Inter's site says it covers "147 languages" and shows Cyrillic glyphs, but does not name them [S30].
- **Licence and Reserved Font Name.** OFL-1.1 lets fonts be bundled with software under another licence and subset or modified, as long as the font stays under OFL and any Reserved Font Name is not used by a modified version [S35]. Noto's OFL.txt declares none [S32]. Inter's website says "Inter" is a Reserved Font Name [S26], but the copyright line in its LICENSE.txt has no name after it [S27]. If the name is reserved, a modified Inter subset may not be called "Inter" and would need another name.
- **Variable fonts.** I did not verify that `ab_glyph` or epaint applies variation axes. Static files avoid the question.
- **What Razdor ships now.** `data/fonts/NotoSans-Regular.ttf` (234,808 bytes) is included with `include_bytes!` in `src/ui/script_font.rs`. The `pub mod script_font;` line in `src/ui/mod.rs` has no cfg gate [local]. Noto is the second font in the fallback chain, after PT Sans and before Noto Sans Devanagari [local]. The file as it is cannot draw Russian. Its Noto-OFL.txt says it is a subset "to the characters Razdor draws with them" [local]. PT Sans draws all 66 Russian letters (measured), so the game's Russian text does not depend on this file.

The studio's theme (`src/ui/studio/mod.rs`, `theme(ctx)`) starts from `egui::Visuals::dark()`. A grep of `src/ui` finds no `set_fonts` or `FontDefinitions` call [local grep], so egui's default fonts draw the studio's text today.

## Open points

- **Ubuntu Font Licence 1.0** (egui's default proportional font, already in every studio build) is not on the FSF's list of licences: a search of the page found no "Ubuntu" entry [S37]. Its GPL compatibility is therefore not verified from a primary source. This needs the owner's decision: keep the default font, or replace it with the chosen one via `FontDefinitions::empty()` (which also drops 361,676 bytes).
- **OFL and GPL.** The FSF entry for OFL-1.1 calls it "a free copyleft license for fonts" and says nothing about GPL compatibility either way [S37]. The OFL FAQ allows fonts to be bundled "alongside other software under FLOSS" and says only the font's own portions need to stay OFL [S35]. I rely on the FAQ for bundling, not on an FSF statement.
- **Inter's Reserved Font Name:** the website says yes [S26]; the LICENSE.txt I read says no [S27]. I could not check the OFL source text on the SIL site.
- **Noto Sans Medium and SemiBold** were not measured; the repo ships only Regular. Weight can be done with size and colour.
- **ab_glyph hinting** is inferred from its description [S13] and epaint's source [S12], not from ab_glyph's own source code.
- **Crispness** was not rendered. No screenshot was taken, and I did not run Razdor.
- **Binary sizes** are file sizes, not the size of compiled output. The resvg code size is unknown.
- **Phosphor version** behind `egui-phosphor` 0.9.0 is not stated; its 1531 constants do not match core 2.1.1's 1512 files [S3][S21].
- **Noto Sans Variable** was only listed from the release zip (1,581,884 bytes [S34]); I did not extract it.
- **Phosphor `layers`:** I did not check whether Phosphor has a `layers` alias; the table uses `stack`.

## Sources

1. https://crates.io/api/v1/crates/egui-macroquad, /versions, /0.17.3/dependencies: egui-macroquad 0.17.3 (MIT/Apache-2.0), dependency `egui` ^0.31.1, `egui-miniquad` ^0.16.0, `macroquad` ^0.4.14. Repository https://github.com/optozorax/egui-macroquad.
2. https://crates.io/api/v1/crates/egui-phosphor/versions and /0.9.0/dependencies, /0.10.0/dependencies, /0.14.0/dependencies: egui-phosphor 0.9.0 requires egui ^0.31; 0.10.0 requires ^0.32; 0.14.0 requires ^0.36; licence MIT OR Apache-2.0. Repository https://github.com/amPerl/egui-phosphor.
3. https://static.crates.io/crates/egui-phosphor/egui-phosphor-0.9.0.crate: Cargo.toml (features `thin`, `light`, `regular`, `bold`, `fill`, default `regular`; no duotone), `src/lib.rs` (`add_to_fonts` inserts the font into the Proportional family), `res/*.ttf` sizes (Regular 488,636; Thin 535,152; Light 536,628; Bold 495,308; Fill 449,252 bytes), `src/variants/regular.rs` constants (1531), README (Phosphor MIT).
4. https://crates.io/api/v1/crates/egui-lucide/versions, /0.2.0/dependencies, /0.1.0/dependencies: egui-lucide 0.2.0 requires egui ^0.37, 0.1.0 requires ^0.36; licence MIT AND ISC. Repository https://github.com/greekfetacheese/egui-lucide.
5. https://crates.io/api/v1/crates/egui-remixicon/0.31.1/dependencies: `egui` is optional. (Not evaluated further.)
6. https://crates.io/api/v1/crates/egui_extras/0.31.1/dependencies and https://static.crates.io/crates/egui_extras/egui_extras-0.31.1.crate (Cargo.toml): egui ^0.31.1; feature `svg = ["resvg"]` with resvg 0.37 optional; licence MIT OR Apache-2.0.
7. https://crates.io/api/v1/crates/resvg/versions: resvg 0.37.0 MPL-2.0; 0.45.1 Apache-2.0 OR MIT, Rust 1.67.1; 0.49.0 Apache-2.0 OR MIT, Rust 1.85.0. Repository https://github.com/linebender/resvg.
8. https://crates.io/api/v1/crates/resvg/0.45.1/dependencies: tiny-skia ^0.11.4, usvg ^0.45.1; optional gif, image-webp, zune-jpeg.
9. https://crates.io/api/v1/crates/egui_dock/0.16.0/dependencies and /versions: egui_dock 0.16.0 requires egui ^0.31; licence MIT.
10. https://static.crates.io/crates/egui/egui-0.31.1.crate (Cargo.toml): `default = ["default_fonts"]`, `default_fonts = ["epaint/default_fonts"]`. egui-macroquad 0.17.3 depends on egui without disabling default features (source 1 and the Cargo.toml in https://static.crates.io/crates/egui-macroquad/egui-macroquad-0.17.3.crate).
11. https://static.crates.io/crates/epaint_default_fonts/epaint_default_fonts-0.31.1.crate: `fonts/Ubuntu-Light.ttf` (361,676 bytes, UFL), `Hack-Regular.ttf` (309,408, OFL), `NotoEmoji-Regular.ttf` (418,804, OFL), `emoji-icon-font.ttf` (317,864, MIT); `UFL.txt` and `OFL.txt`. Crate page https://crates.io/api/v1/crates/epaint_default_fonts.
12. https://raw.githubusercontent.com/emilk/egui/0.31.1/crates/epaint/src/text/font.rs: `ab_glyph` `outline_glyph` and `glyph.draw`; no hinting; rounding of font sizes and vertical offsets to the pixel grid.
13. https://github.com/alexheretic/ab-glyph: Apache-2.0; "Zero dependency coverage rasterization for lines, quadratic & cubic beziers" (the README does not discuss hinting). Also the lock entry `ab_glyph` 0.2.32 in the probe lockfile.
14. https://github.com/lucide-icons/lucide/blob/main/LICENSE: ISC License, Copyright (c) 2026 Lucide Icons and Contributors; a separate MIT License for Feather-derived icons.
15. https://registry.npmjs.org/lucide-static/latest: version 1.68.0, fileCount 6382.
16. https://data.jsdelivr.com/v1/package/npm/lucide-static@1.68.0/flat: 2313 SVG icons under `/icons`; `/font/lucide.ttf` 980,804 bytes, `/font/codepoints.json` (2333 names, fetched from https://cdn.jsdelivr.net/npm/lucide-static@1.68.0/font/codepoints.json).
17. https://cdn.jsdelivr.net/npm/lucide-static@1.68.0/icons/<name>.svg: HTTP 200 for every Lucide name in the coverage table, and their byte sizes.
18. https://cdn.jsdelivr.net/npm/lucide-static@1.68.0/LICENSE: ISC License, Copyright (c) 2026 Lucide Icons and Contributors.
19. https://github.com/phosphor-icons/core: MIT, package `@phosphor-icons/core` (WebFetch of the repository page; the licence line is confirmed by source 23).
20. https://registry.npmjs.org/@phosphor-icons/core/latest: version 2.1.1.
21. https://data.jsdelivr.com/v1/package/npm/@phosphor-icons/core@2.1.1/flat: 1512 SVG files in each of `assets/thin`, `light`, `regular`, `bold`, `fill`, `duotone`.
22. https://cdn.jsdelivr.net/npm/@phosphor-icons/core@2.1.1/assets/regular/<name>.svg: HTTP 200 for every Phosphor name in the coverage table (`selection-plain` returned 404 and is not used); byte sizes. `lock-open` also exists.
23. https://cdn.jsdelivr.net/npm/@phosphor-icons/core@2.1.1/LICENSE: MIT License, Copyright (c) 2023 Phosphor Icons.
24. https://registry.npmjs.org/@tabler/icons/latest and https://data.jsdelivr.com/v1/package/npm/@tabler/icons@3.50.0/flat: version 3.50.0, MIT; 5203 outline and 1054 filled SVG icons.
25. https://cdn.jsdelivr.net/npm/@tabler/icons@3.50.0/LICENSE: MIT License, Copyright (c) 2020-2026 Pawel Kuna.
26. https://github.com/rsms/inter (WebFetch): SIL Open Font License 1.1; states `"Inter" is a Reserved Font Name by Rasmus Andersson`; variable and static files; no hinting statement. https://rsms.me/inter/ (see source 30).
27. https://raw.githubusercontent.com/rsms/inter/v4.1/LICENSE.txt: "Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter)", the standard OFL 1.1 text, with no Reserved Font Name declared after the copyright line.
28. https://api.github.com/repos/rsms/inter/releases/latest: tag v4.1, published 2024-11-16.
29. https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip: InterVariable.ttf 879,708 bytes; extras/ttf/Inter-Regular.ttf 411,640; extras/ttf/Inter-Medium.ttf 417,300; extras/ttf/Inter-SemiBold.ttf 419,744; extras/woff-hinted/ (WOFF2 hinted variants). Downloaded and inspected (table directory and cmap read with a small script in scratch).
30. https://rsms.me/inter/ (WebFetch): "features over 2000 glyphs covering 147 languages", a glyph grid that shows Cyrillic and Greek characters.
31. https://github.com/notofonts/latin-greek-cyrillic (WebFetch): licence SIL OFL 1.1; the title names Latin, Greek and Cyrillic.
32. https://raw.githubusercontent.com/notofonts/latin-greek-cyrillic/main/OFL.txt: "Copyright 2022 The Noto Project Authors (https://github.com/notofonts/latin-greek-cyrillic)", the OFL 1.1 text, no Reserved Font Name declared.
33. https://api.github.com/repos/notofonts/latin-greek-cyrillic/releases/latest: tag NotoSans-v2.015, published 2024-11-20.
34. https://github.com/notofonts/latin-greek-cyrillic/releases/download/NotoSans-v2.015/NotoSans-v2.015.zip: `NotoSans/unhinted/ttf/NotoSans-Regular.ttf` 431,364 bytes; `NotoSans/hinted/ttf/NotoSans-Regular.ttf` 621,572; `NotoSans/unhinted/variable-ttf/NotoSans[wdth,wght].ttf` 1,581,884 (listed only); `OFL.txt`. Regular extracted and inspected: Cyrillic 304 of 304, Russian 66 of 66, no hint tables.
35. https://openfontlicense.org/ofl-faq/: a Reserved Font Name is optional and applies to modified versions; fonts "can be freely included alongside other software under FLOSS"; "Only the portions based on the Font Software are required to be released under the OFL"; the font itself stays under OFL.
36. https://github.com/tabler/tabler-icons: the Tabler repository, named in the @tabler/icons package metadata (source 24); not fetched.
37. https://www.gnu.org/licenses/license-list.html: MIT/X11 "compatible with the GNU GPL"; ISC "compatible with the GNU GPL"; Apache-2.0 "compatible with version 3 of the GNU GPL"; MPL-2.0 section 3.3 gives compatibility with GPL version 2 and later versions; SIL OFL 1.1 "is a free copyleft license for fonts" with no compatibility statement; no entry found for the Ubuntu Font Licence.
38. https://crates.io/api/v1/crates/egui-tabler: "crate `egui-tabler` does not exist".
39. https://crates.io/api/v1/crates/egui-phosphor and https://crates.io/api/v1/crates/egui-lucide: repository and description fields (already in source 2 and 4).
40. https://pypi.org/project/fonttools/: fonttools 4.68.0, installed with `pip --no-deps` into the scratch directory and used for subsetting (`fontTools.subset`).
41. Local measurements (no URL): `cargo tree` on lockfiles resolved from the crates.io index (cargo 1.90.0) for `egui-macroquad` 0.17.3, `egui-phosphor` 0.9, `egui_dock` 0.16 and `egui_extras` 0.31 with `svg`; 211 locked packages in the base set. Probe directories deleted afterwards.
42. Local repo files (this worktree): `Cargo.toml` lines 16, 38-39, 70 (macroquad, egui-macroquad, egui_dock, feature `studio`); `src/ui/mod.rs` line 46 (`pub mod script_font;`, no gate); `src/ui/script_font.rs` lines 25-31 and 289-293 (`NOTO_SANS`, fallback order); `data/fonts/Noto-OFL.txt`; `src/ui/studio/mod.rs` `theme(ctx)` around line 1302 (`egui::Visuals::dark()`).

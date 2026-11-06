# Bundled fonts

DM Sans Regular is the primary proportional UI font, with explicit variable weight 400 and optical size 14 for small UI text. Existing egui fonts remain fallbacks. Monospaced previews retain their existing primary font, with DM Sans as a fallback for missing symbols.

## Sources

`DMSans.ttf` is the unmodified `DMSans[opsz,wght].ttf` from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/dmsans), downloaded November 6, 2026. Its SIL Open Font License is in `DMSans-OFL.txt`.

The bundled font's character map was verified on Windows to contain U+2192 (right arrow), U+2190 (left arrow), and U+2026 (ellipsis). Fonts are embedded in the executable; no system installation or runtime download is needed.

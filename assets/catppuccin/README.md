# Catppuccin Mocha file icons

Vendored from [Catppuccin Icons](https://github.com/catppuccin/vscode-icons), release **v1.26.0**, under the included MIT license.

Source package: https://github.com/catppuccin/vscode-icons/releases/download/v1.26.0/catppuccin-vsc-icons-1.26.0.vsix

Package SHA-256: `57566136d0a8ba8d040eb6e5be866c88ccbb04f3d219d3a7e52a19978900ff3d`.

The 390 SVGs are the unmodified Mocha file icons referenced by the upstream filename and extension mappings. Folder and language-ID-only associations are omitted because search results are files and do not use an editor language detector. Unknown types use `_file.svg`.

To regenerate, extract that VSIX as a ZIP, then run `scripts/import-icons.ps1 -ExtensionDirectory <extracted-extension-directory>`. This copies assets and licensing, exports `mapping.json`, and generates `src/file_icon_assets.rs`. No download or generator is needed for a normal build.

The application embeds the assets, matches exact filenames before longest compound extensions (case-insensitive), and uses egui's native SVG loader/cache. No webview, shell icon query, or filesystem access occurs during icon selection.

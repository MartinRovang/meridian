# assets

- `logo.png` — the circular artwork. Splash art (`src/Splash.tsx`) and, via
  `cargo tauri icon assets/logo.png`, the desktop icon at every size.
- `banner.png` — the wide 2:1 artwork. README header and release-notes header.

Both are the supplied artwork. Replacing either is a drop-in at the same path.

When `logo.png` is replaced, regenerate the desktop icon set from it:

```bash
cargo tauri icon assets/logo.png
rm -rf src-tauri/icons/android src-tauri/icons/ios src-tauri/icons/Square*.png src-tauri/icons/StoreLogo.png
```

The second line drops the mobile and Windows-Store sizes the CLI also emits;
nothing in `tauri.conf.json` references them.

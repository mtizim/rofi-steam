# rofi-steam

Small `rofi` launcher for installed Steam games.

## Behavior

- Reads installed games from `~/.steam`.
- Uses `~/.launchablegames` as a cache.
- If cache exists, shows cached data immediately and refreshes cache in the background.
- If cache is missing/empty, scans Steam first, writes cache, then shows menu.
- Launches with `steam://rungameid/<appid>`.

## Build

```bash
cargo build --release
```

## Install

```bash
mv target/release/rofi-lutris ~/.config/sway/rofi-lutris
chmod +x ~/.config/sway/rofi-lutris
```

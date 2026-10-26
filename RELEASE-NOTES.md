# slot. for KONKR Pocket Advance — v1.3.3

A big update: a second core for Game Boy and Game Boy Color, new screen sizes and overlays, sharper visuals at the KONKR's native resolution, and controls that respond like RetroArch.

### What's New

- **Game Boy and Game Boy Color multiplayer** — Two KONKRs can now link GB and GBC games over Wi-Fi, with mGBA or Gambatte. In a game, press SELECT + HOME, then choose Host on one console and Join on the other. Any game links with the same ROM on both sides, and Pokémon games of the same region can trade and battle with each other. mGBA keeps both consoles in perfect sync, which suits fast two-player games best.
- **Gambatte core for Game Boy and Game Boy Color** — GB and GBC games can now run on Gambatte as well as mGBA. Press SELECT on a cartridge to choose the core for that game. Your saves follow you when you switch cores.
- **Default core per system** — Settings → Systems now has a page for each system (GBA, GBC, GB) with a Default Core option. You can still override it for any game with SELECT.
- **New GB/GBC screen sizes** — L1 and R1 switch between Integer, Original Ratio and Stretch.
- **Game Boy overlays** — Game Boy and Game Boy Pocket overlays for GB games, and a Game Boy Color overlay for GBC games. Made by [Watomsk](https://github.com/Watomsk/Overlays), adapted to the KONKR's 3:2 screen.
- **Live GB Palette** — The palette changes in real time while you browse them, as in slot. 1.8.1.
- **Sharper visuals** — Cartridges, labels, icons and menus are now rendered at the KONKR's native 960x640 resolution.
- **More responsive controls** — The D-pad (hat) is read directly with no buffering, quick taps between two frames are no longer lost, and inputs are polled at a steady point in each frame, as in RetroArch. Audio output latency is lower too.
- **Adaptive Link Cable delay** — The mGBA Link Cable input delay now adjusts automatically to the connection quality between the two consoles.
- **Faster game launch** — Games start quickly even after idling on the shelf, and the first scroll after ejecting a game is smooth.

### Fixed in 1.3.3

- **Stuck screen after an update** — After updating from inside slot., the "App installed" screen no longer gets stuck (OPEN did nothing, and with slot. as the home app the console had to be restarted). The fix applies to updates installed from 1.3.3 onward.

### Fixed in 1.3.2

- **RetroAchievements on GBA** — Some games, such as Super Mario Advance, showed "Core does not expose achievement RAM" at launch when Run-Ahead was on. Achievements now work with Run-Ahead.
- **Pokémon Emerald achievements** — Games with a large achievement set, such as Pokémon Emerald, showed "Achievement tracking interrupted" right after launch. Tracking now starts cleanly.
- **BIOS message at startup** — With slot. as the home app, "Couldn't import BIOS files" no longer appears after restarting the console. Your BIOS files were never lost.
- **Controls after sleep** — Buttons now respond as soon as the console wakes up, with no 2-second delay or grey screen.

### Fixed in 1.3.1

- **Pokémon save error on exit** — On mGBA, Pokémon FireRed, LeafGreen, Ruby, Sapphire and Emerald showed "Couldn't save" when ejecting a game that had not been saved in-game yet. Save and Eject now works, and an existing in-game save is still protected.

### Installation

Download the APK below and install it over your existing version. Your games, saves and settings are preserved.

### Credits

An unofficial Android port of [slot.](https://slot-cfw.fyi/) by [Brandon Kowalski](https://github.com/BrandonKowalski/slot) and contributors. Game Boy overlays by [Watomsk](https://github.com/Watomsk/Overlays). Gambatte by Sindre Aamås and contributors.

Licensed under GPL-3.0.

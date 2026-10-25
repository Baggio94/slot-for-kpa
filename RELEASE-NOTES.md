# slot. for KONKR Pocket Advance — v1.3.1

A big update: a second core for Game Boy and Game Boy Color, new screen sizes and overlays, sharper visuals at the KONKR's native resolution, and controls that respond like RetroArch.

### What's New

- **Gambatte core for Game Boy and Game Boy Color** — GB and GBC games can now run on Gambatte as well as mGBA. Press SELECT on a cartridge to choose the core for that game: the cartridge opens on its pak board and you pick the chip, just like mGBA/gpSP on GBA. Your saves follow you when you switch cores.
- **Default core per system** — Settings → Systems now has a page for each system (GBA, GBC, GB) with a Default Core option. You can still override it for any game with SELECT.
- **New GB/GBC screen sizes** — L1 and R1 switch between Integer, Original Ratio (the real 10:9 shape, kept sharp) and Stretch.
- **Game Boy overlays** — Game Boy and Game Boy Pocket overlays for GB games, and a Game Boy Color overlay for GBC games. Made by [Watomsk](https://github.com/Watomsk/Overlays), adapted to the KONKR's 3:2 screen and included with permission.
- **Live GB Palette** — The palette changes in real time while you browse them, as in slot. 1.8.1.
- **Sharper visuals** — Cartridges, labels, icons and menus are now rendered at the KONKR's native 960x640 resolution.
- **More responsive controls** — The D-pad (hat) is read directly with no buffering, quick taps between two frames are no longer lost, and inputs are polled at a steady point in each frame, as in RetroArch. Audio output latency is lower too.
- **Adaptive Link Cable delay** — The mGBA Link Cable input delay now adjusts automatically to the connection quality between the two consoles.
- **Faster game launch** — Games start quickly even after idling on the shelf, and the first scroll after ejecting a game is smooth.

### Fixed in 1.3.1

- **Pokémon save error on exit** — On mGBA, Pokémon FireRed, LeafGreen, Ruby, Sapphire and Emerald showed "Couldn't save" when ejecting a game that had not been saved in-game yet. Save and Eject now works, and an existing in-game save is still protected.

### Installation

Download the APK below and install it over your existing version. Your games, saves and settings are preserved.

### Credits

An unofficial Android port of [slot.](https://slot-cfw.fyi/) by [Brandon Kowalski](https://github.com/BrandonKowalski/slot) and contributors. Game Boy overlays by [Watomsk](https://github.com/Watomsk/Overlays). Gambatte by Sindre Aamås and contributors.

Licensed under GPL-3.0.

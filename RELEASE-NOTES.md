# slot. for Android — v2.1

Your controllers, your shortcuts, and multiplayer over the Internet. The repository is now **slot-for-android**.

### What's New

- **Multiplayer over the Internet (beta)** — Next to LAN, play over the Internet (WAN) with a four-digit room code: Host shows the code, Y shares it, your friend types it with the D-pad. No router setup. Every Link mode works except Gambatte's Game Boy link, which stays on LAN for now.
- **Settings → Controls** — Choose the controller of Port 1 and Port 2 and remap its buttons. With a second controller, two players share one handheld on NES, Super Nintendo and Mega Drive.
- **Hotkeys** — Change every shortcut, and choose SELECT, L3 or R3 as the Hotkey Button (L3 and R3 only where the handheld has them). Fast-forward (R2), rewind (L2) and the Boktai sun keep their buttons.
- **Controls per system and per game** — In Systems → [system] → Controls, choose which button plays each console button, and what X / Y and L1 / R1 do on consoles without them. In a game, Settings → Controls changes it for that game only.
- **Screen menu** — SELECT + Y opens it straight from the game. The game stays in view while you change its shader, color correction, palette and overlay, and L1 / R1 change the screen size on every system. The slot rises with it, as in the original slot.
- **Frame Blending** — Off, Mix or Ghosting for each system, for games that flicker their sprites to draw shadows or transparency.
- **Faster in-game menu** — The menu shows sooner, and a double tap on the slot. button opens Save States without the menu flashing first.
- **Menus for every screen** — Six rows at most, the rest scrolls; on narrow screens a setting's name makes room for its value.

### Changed

- SELECT + Y now opens the Screen menu. Color Correction has no shortcut by default: give it one in Hotkeys, or change it in the Screen menu.
- Settings: Controls comes after Emulation.
- Multiplayer says LAN and WAN instead of Local and Internet.

### Fixed

- Grid draws its lines as the original slot., even at any scale.
- LCD Grid V2 blends its subpixels as in RetroArch.
- AGB-001 and AGS-001 keep even subpixel columns at any scale.
- B in Save States opened from the in-game menu returns to the menu.
- Settings → Systems pages show their system's name.
- No more "LABELS" flash after ejecting a cartridge.
- Multiplayer keycaps fit 4:3 and narrower screens.

### Installation

Install the APK below over your existing version: your games, saves and settings are kept. On 2.0, slot. also offers this update from **START → About**. On 1.3, install this APK by hand once; updates from **START → About** work again from there.

### Credits

An unofficial Android port of [slot.](https://slot-cfw.fyi/) by [Brandon Kowalski](https://github.com/BrandonKowalski/slot) and contributors. Overlays by [Watomsk](https://github.com/Watomsk/Overlays). Emulation by mGBA, gpSP, Gambatte, FCEUmm, Nestopia, Snes9x, bsnes, Genesis Plus GX and PicoDrive. Labels from ScreenScraper.

Licensed under GPL-3.0.

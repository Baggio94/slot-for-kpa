#!/usr/bin/env python3
"""Package pinned original Cart Studio with only the KONKR SAF adapter.

Must run after official wasm-pack compilation. No emulation/runtime files,
original renderer, fonts, sprites, or Studio UI logic are rewritten.
"""
from pathlib import Path
import argparse
import shutil

def checked_replace(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError("Official Studio changed; review Android adapter: " + old[:90])
    return text.replace(old, new)

def package(root: Path, built: Path):
    source = root / "third_party/slot-cart-studio/web"
    target = root / "android/app/src/main/assets/studio"
    assert (source / "studio.js").is_file()
    assert (built / "pkg/slot_cart_studio_bg.wasm").is_file()
    target.mkdir(parents=True, exist_ok=True)

    for file in source.glob("*"):
        if file.is_file() and file.suffix in {".js", ".css", ".html"}:
            shutil.copy2(file, target / file.name)
    (target / "pkg").mkdir(exist_ok=True)
    for name in ("slot_cart_studio.js", "slot_cart_studio_bg.wasm"):
        shutil.copy2(built / "pkg" / name, target / "pkg" / name)
    shutil.copy2(root / "android/studio/android-card.js", target / "android-card.js")
    # Only the Android-packaged web app gets compact embedded CSS.
    shutil.copy2(root / "android/studio/embedded.css", target / "embedded.css")

    jsfile = target / "studio.js"
    js = jsfile.read_text("utf-8")
    js = "import { fromAndroid } from './android-card.js';\n" + js
    js = checked_replace(js, "  const crc = new Crc32();",
                         "  if (file.slotCrc != null) return file.slotCrc >>> 0;\n  const crc = new Crc32();")
    # Keep Android's opaque per-ROM identifier on the official cart objects;
    # otherwise the selected X shortcut cannot find its cart after newCart().
    js = checked_replace(js,
        "function newCart({ platform, stem, file }) {",
        "function newCart({ platform, stem, file, slotId = '' }) {")
    js = checked_replace(js,
        "    platform,\n    stem,\n    file,\n    code: '',",
        "    platform,\n    stem,\n    file,\n    slotId,\n    code: '',")
    # Selected cart (X) enters its editor immediately, while the normal menu
    # displays the official three-tab catalog with Real Label/Logo Only prompt.
    # Selected-first order also makes ROM matching responsive in a large library.
    js = checked_replace(js,
                         "  showPlatform(platformsOf(session)[0] ?? PLATFORMS[0]);",
                         """  const selected = source.selectedKey &&
      session.carts.find((c) => c.slotId === source.selectedKey);
  showPlatform(selected?.platform ?? platformsOf(session)[0] ?? PLATFORMS[0]);
  if (window.AndroidStudio && selected) editor.open(selected);""")
    js = checked_replace(js, "  document.body.dataset.ready = 'true';" ,
        """  document.body.dataset.ready = 'true';
  if (window.AndroidStudio) {
    $('pick').hidden = true;
    $('pick-files-label').hidden = true;
    try {
      await open(fromAndroid(window.AndroidStudio));
    } catch (error) {
      console.error('KONKR Cart Studio:', error);
      banner('Could not open selected cartridge: ' + error.message);
    }
  }""")
    # Reuse Slot's native No-Intro matcher only after the Studio's ordinary
    # exact CRC lookup failed. Keep the chosen official CRC for art retrieval.
    js = checked_replace(js,
        "    c.game = dats.get(c.platform)?.game_for(c.crc) ?? null;\n    await dress(s, c);",
        """    c.game = dats.get(c.platform)?.game_for(c.crc) ?? null;
    if (!c.game && window.AndroidStudio && c.slotId) {
      const matched = window.AndroidStudio.officialMatch(c.slotId);
      if (/^[0-9A-F]{8}$/.test(matched)) {
        const official = parseInt(matched, 16);
        const game = dats.get(c.platform)?.game_for(official) ?? null;
        if (game) {
          c.game = game;
          c.androidOfficialCrc = official;
        }
      }
    }
    await dress(s, c);""")
    js = checked_replace(js,
        "function artKey(c) {\n  const own = c.crc === null ? null : crcHex(c.crc);",
        """function artKey(c) {
  if (window.AndroidStudio && c.androidOfficialCrc != null) {
    const original = crcHex(c.androidOfficialCrc);
    if (artIndex?.[original]) return original;
  }
  const own = c.crc === null ? null : crcHex(c.crc);""")
    # Load the official art index concurrently with local library setup:
    # the three tabs and edited cartridge remain usable with slow/offline Wi-Fi.
    # Browser-hosted Cart Studio already downloads from this art set,
    # but it may CORS-block our embedded Android origin. Use the same files
    # through a tightly allowlisted native proxy under our own HTTPS origin.
    js = checked_replace(js,
        "const ART_BASE = new URLSearchParams(location.search).get('art') ?? 'https://art.slot-cfw.fyi/';",
        "const ART_BASE = window.AndroidStudio ? new URL('art/', location.href).href : (new URLSearchParams(location.search).get('art') ?? 'https://art.slot-cfw.fyi/');")
    js = checked_replace(js, "let artIndex = null;",
        "let artIndex = null;\nlet androidIndexReady = Promise.resolve();")
    js = checked_replace(js,
        "async function match(s) {\n  // One database per platform",
        "async function match(s) {\n  await androidIndexReady;\n  // One database per platform")
    js = checked_replace(js,
        """  if (ART_BASE) {
    try {
      artIndex = await fetchIndex(ART_BASE);
      console.info(`art set: ${Object.keys(artIndex).length} checksums from ${ART_BASE}`);
    } catch (e) {
      banner(`The art set at ${ART_BASE} didn’t load (${e.message}), so carts get slot’s own labels.`);
    }
  }""",
        """  if (ART_BASE) {
    const loadIndex = async () => {
      try {
        artIndex = await fetchIndex(ART_BASE);
        console.info(`art set: ${Object.keys(artIndex).length} checksums from ${ART_BASE}`);
      } catch (e) {
        banner(`The art set at ${ART_BASE} didn’t load (${e.message}), so carts get slot’s own labels.`);
      }
    };
    if (window.AndroidStudio) {
      // Never block entering the editor on a remote service.
      androidIndexReady = Promise.race([
        loadIndex(),
        new Promise((resolve) => setTimeout(resolve, 12000)),
      ]);
    } else {
      await loadIndex();
    }
  }""")
    # Do not show the same "Reading 1 of 125..." blocking screen after
    # the prior scan produced validated cached fingerprints. A new/modified
    # ROM still runs the original progress pipeline.
    js = checked_replace(js,
        "  loading('Reading the card…');",
        "  if (!window.AndroidStudio || !source.allCached) loading('Reading the card…');")
    js = checked_replace(js,
        "function progress(done, total, stem, verb = 'Reading') {",
        """function progress(done, total, stem, verb = 'Reading') {
  if (window.AndroidStudio && session?.source?.allCached && verb === 'Reading') {
    $('progress').hidden = true;
    return;
  }""")
    js = checked_replace(js,
        "  loading('Looking up the games…');",
        "  if (!window.AndroidStudio || !s.source.allCached) loading('Looking up the games…');")
    # Original Cart Studio does not compute a CRC for carts with an existing
    # label, but we can reuse their previously stored CRC without reading ROMs.
    # This lets the X editor recognize a previously decorated game again.
    js = checked_replace(js,
        "        c.existingPng = bytes;\n        c.choice = 'card';",
        """        c.existingPng = bytes;
        if (window.AndroidStudio && file.slotCrc != null) c.crc = file.slotCrc;
        c.choice = 'card';""")
    js = checked_replace(js,
        "  const pending = s.carts.filter((c) => c.crc !== null && !c.game && !c.existing && !c.error);",
        """  if (window.AndroidStudio) for (const c of s.carts) {
    if (c.existing && c.crc !== null && !c.game) {
      c.game = dats.get(c.platform)?.game_for(c.crc) ?? null;
      if (c.game) paint(c);
    }
  }
  const pending = s.carts.filter((c) => c.crc !== null && !c.game && !c.existing && !c.error);""")
    # X focuses one game: never ask the bulk Real Label / Logo Only wizard.
    js = checked_replace(js,
        "if (s === session && !fatal && s.fill == null) await fillOrAsk(s);",
        "if (s === session && !fatal && s.fill == null && !s.source.focused) await fillOrAsk(s);")
    js = checked_replace(js, "let session = null;",
        """let session = null;
let lastNativeActionState = '';
function syncNativeToolbar() {
  if (!window.AndroidToolbar || !session) return;
  const save = writing || (!$('write-bar').hidden && !$('write').disabled);
  const fill = !$('fill-open').hidden;
  const state = String(save) + ':' + String(fill);
  if (state === lastNativeActionState) return;
  lastNativeActionState = state;
  window.AndroidToolbar.actions(save, fill);
}""")
    # WebView render budget: never compose 125 offscreen cart PNG canvases.
    # The official renderer/label functions stay untouched. Every visible card
    # is painted as it enters the viewport; hidden tabs do no canvas work.
    js = checked_replace(js, "let lastNativeActionState = '';",
        """let lastNativeActionState = '';
let androidCartObserver = null;
function observeAndroidCarts(s) {
  if (!window.AndroidStudio || !('IntersectionObserver' in window)) return;
  androidCartObserver?.disconnect();
  androidCartObserver = new IntersectionObserver((entries) => {
    if (s !== session) return;
    for (const entry of entries) {
      const c = entry.target.slotCart;
      if (!c) continue;
      c.androidVisible = entry.isIntersecting;
      if (c.androidVisible) schedulePaint(c);
    }
  }, { rootMargin: '200px 0px' });
  for (const c of s.carts) {
    c.androidVisible = false;
    c.el.root.slotCart = c;
    androidCartObserver.observe(c.el.root);
  }
}""")
    js = checked_replace(js,
        "  $('grid').replaceChildren(...session.carts.map(buildCard));",
        "  $('grid').replaceChildren(...session.carts.map(buildCard));\n  observeAndroidCarts(session);")
    js = checked_replace(js,
        "function paint(c) {\n  if (fatal) return;",
        """function paint(c) {
  if (fatal) return;
  if (window.AndroidStudio && androidCartObserver &&
      !c.androidVisible && editor.current() !== c) {
    const state = stateOf(c);
    c.el.root.dataset.state = state;
    c.el.game.textContent = describe(c, state);
    c.el.print.textContent = smallPrint(c.platform, c.code, c.tags);
    updateWriteBar();
    updateFillButton();
    return;
  }""")
    # During a large automatic-fill save, reuse original per-cart progress
    # instead of leaving the native Save to slot. button unresponsive.
    js = checked_replace(js,
        "  $('write').style.setProperty('--done', total > 0 ? String(done / total) : '0');",
        "  $('write').style.setProperty('--done', total > 0 ? String(done / total) : '0');\n  if (window.AndroidToolbar) window.AndroidToolbar.progress(done, total);")
    js = checked_replace(js,
        "  $('write').textContent = session.source.direct ? 'Write to card' : 'Download ZIP';",
        "  $('write').textContent = session.source.direct ? 'Save to slot.' : 'Download ZIP';\n  syncNativeToolbar();")
    js = checked_replace(js,
        "  $('fill-open').hidden = counts.unlabelled === 0 || (session.fill != null && autoFill(counts) !== null);",
        "  $('fill-open').hidden = counts.unlabelled === 0 || (session.fill != null && autoFill(counts) !== null);\n  syncNativeToolbar();")
    js = checked_replace(js,
        "          logError(c.stem, e);\n          c.result = 'failed';",
        """          logError(c.stem, e);
          if (window.AndroidStudio) banner(c.stem + ': ' + e.message);
          c.result = 'failed';""")
    # Shell configuration writes are part of the same total. A final
    # progress callback is required even when only shells were edited.
    js = checked_replace(js,
        "      // One that didn't, all of it, says so. Not over a card opened while this one was writing.",
        """      writeProgress(carts.length + shells.length, carts.length + shells.length);
      // One that didn't, all of it, says so. Not over a card opened while this one was writing.""")
    # The official reminder is for a physical card removed from a computer.
    # Android writes via its scoped SAF bridge and must not imply the user
    # needs to eject a physical SD card. Keep the reminder for web users.
    js = checked_replace(js,
        "        $('eject').hidden = !(count.written || count.shells);",
        "        $('eject').hidden = window.AndroidStudio || !(count.written || count.shells);")
    js = checked_replace(js,
        "        if (problems.length) banner(problems.join(' '));",
        """        if (problems.length) {
          // The Android bridge already showed the precise game + write error.
          // Do not overwrite it with a generic "1 label couldn't be written".
          if (!window.AndroidStudio || count.failed === 0) banner(problems.join(' '));
        }
        if (window.AndroidToolbar) {
          // Exactly one result. Include shell-only saves and skipped/failed
          // labels; keep per-cart technical details in the Studio banner/log.
          window.AndroidToolbar.summary(
            count.written, count.skipped, count.failed,
            shellNote ? 0 : shells.length, shellNote
          );
        }""")
    editorFile = target / "editor.js"
    ed = editorFile.read_text("utf-8")
    ed = checked_replace(ed, "    $('ed-query').value = api.name(cart);",
        r"""    $('ed-query').value = window.AndroidStudio
      ? api.name(cart).replace(/\s*\([^)]*\)/g, '').trim()
      : api.name(cart);""")
    ed = checked_replace(ed,
        "    renderTiles(c);\n\n    const bg = api.background(c);",
        "    renderTiles(c);\n    if (window.AndroidStudio && !$('ed-games').hidden) showGames();\n\n    const bg = api.background(c);")
    editorFile.write_text(ed, encoding="utf-8")
    # Original libretro DAT URLs via native cache on Android only.
    datFile = target / "libretro.js"
    dat = datFile.read_text("utf-8")
    dat = checked_replace(dat,
        "  const r = await fetch(`${DAT}${encodeURIComponent(NAMES[platform])}.dat`);",
        "  const r = await fetch(window.AndroidStudio ? '/studio/dat/' + platform + '.dat' : `${DAT}${encodeURIComponent(NAMES[platform])}.dat`);")
    datFile.write_text(dat, encoding="utf-8")


    js = js.replace("'Write to card'", "'Save to slot.'")
    jsfile.write_text(js, encoding="utf-8")

    htmlfile = target / "index.html"
    html = htmlfile.read_text("utf-8")
    html = checked_replace(html, '<meta charset="utf-8">',
        """<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="
 default-src 'self';
 script-src 'self' 'wasm-unsafe-eval';
 style-src 'self' 'unsafe-inline' https://fonts.googleapis.com;
 font-src 'self' https://fonts.gstatic.com data:;
 connect-src 'self' https://raw.githubusercontent.com https://art.slot-cfw.fyi;
 img-src 'self' data: blob: https://raw.githubusercontent.com https://art.slot-cfw.fyi;
 object-src 'none'; frame-src 'none'; base-uri 'self'; form-action 'none';
">""")
    html = checked_replace(html, '<link rel="stylesheet" href="studio.css">',
        '<link rel="stylesheet" href="studio.css">\n<link rel="stylesheet" href="embedded.css">')
    htmlfile.write_text(html, encoding="utf-8")
    assert 'editor.open(selected)' in js
    assert 'embedded.css' in html
    assert (target / 'embedded.css').is_file()
    print("Original Cart Studio packaged:", target,
          (target / "pkg/slot_cart_studio_bg.wasm").stat().st_size, "WASM bytes")

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=".")
    ap.add_argument("--built", required=True)
    args = ap.parse_args()
    package(Path(args.source), Path(args.built))

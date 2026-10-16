# Slot. for KPA — Android Icon Pack v3 (corrigé)

Pack corrigé pour distinguer clairement :
- l'icône **Google Play**
- les ressources **launcher Android / adaptive icon**

## Conformité ciblée
Basé sur la page Android Developers :
- Google Play icon : **512 × 512 px**, **PNG 32 bits**, **sRVB**, **carré plein**, **sans coins arrondis**, **sans ombre**.
- Launcher Android : icône adaptative avec **foreground** séparé, **background** séparé et **monochrome**.

## Contenu
### `source/`
- `slot_kpa_brand_master.svg` : master vectoriel de la cartouche
- `slot_kpa_adaptive_foreground_safe.svg` : foreground SVG avec safe area corrigée
- `slot_kpa_adaptive_monochrome_safe.svg` : version monochrome SVG
- `slot_kpa_launcher_preview.svg` : preview launcher (fond arrondi, pour présentation)
- `slot_kpa_play_store_square.svg` : version SVG Google Play (fond carré)
- `slot_kpa_material_you_preview.svg` : preview Material You

### `exports/`
- `play_store_512.png` : à utiliser pour la fiche Google Play
- `launcher_preview_1024.png`
- `adaptive_foreground_1024.png`
- `adaptive_monochrome_1024.png`
- `material_you_preview_1024.png`

### `android/`
- `mipmap-anydpi-v26/ic_launcher.xml`
- `mipmap-anydpi-v26/ic_launcher_round.xml`
- `values/ic_launcher_colors.xml`
- `mipmap-*/ic_launcher.png` et `ic_launcher_round.png` (legacy PNG)
- `drawable-source/` : sources SVG + previews PNG à importer/convertir dans Android Studio

## Recommandation d'intégration
1. Utiliser `exports/play_store_512.png` pour **Google Play**.
2. Utiliser `android/mipmap-anydpi-v26/` + `ic_launcher_background` pour le **launcher**.
3. Importer `android/drawable-source/ic_launcher_foreground_source.svg` en `@drawable/ic_launcher_foreground`.
4. Importer `android/drawable-source/ic_launcher_monochrome_source.svg` en `@drawable/ic_launcher_monochrome`.
5. Vérifier le rendu sur le launcher du KONKR Pocket Advance (Android 12) et sur masques ronds / carrés arrondis.

## Palette
- Dark: `#141B23`
- Cream: `#F0EAD8`
- Beige: `#EFE1CA`
- Taupe: `#7A6A5D`

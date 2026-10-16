//! The No-Intro GBA DAT as libretro-database publishes it: clrmamepro text, one `game ( ... )`
//! block per dump, with the dump's CRC32 on its `rom ( ... )` line.

use std::collections::HashMap;

pub struct Dat {
    by_crc: HashMap<u32, String>,
}

impl Dat {
    /// Lines outside a game block, such as the `clrmamepro` header, name nothing that has a CRC.
    /// A CRC listed twice keeps its first name.
    pub fn parse(text: &str) -> Self {
        let mut by_crc = HashMap::new();
        let mut name: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("game (") {
                name = None;
            } else if let Some(rest) = line.strip_prefix("name \"") {
                name = rest.rfind('"').map(|end| rest[..end].to_string());
            } else if line.starts_with("rom (") {
                let crc = line
                    .split_whitespace()
                    .skip_while(|token| *token != "crc")
                    .nth(1)
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok());
                if let (Some(crc), Some(name)) = (crc, &name) {
                    by_crc.entry(crc).or_insert_with(|| name.clone());
                }
            }
        }
        Self { by_crc }
    }

    pub fn game_for(&self, crc: u32) -> Option<&str> {
        self.by_crc.get(&crc).map(String::as_str)
    }

    /// Every dump filed under `name`, lowest CRC first, so a game chosen by name can be found in
    /// a set keyed by checksum.
    pub fn crcs_for(&self, name: &str) -> Vec<u32> {
        let mut crcs: Vec<u32> = self
            .by_crc
            .iter()
            .filter(|(_, n)| n.as_str() == name)
            .map(|(crc, _)| *crc)
            .collect();
        crcs.sort_unstable();
        crcs
    }

    /// Games whose name contains `query`, ignoring case, for a cart whose CRC matched the wrong
    /// game or nothing at all. Names starting with the query come first — someone typing "zelda"
    /// wants the Zelda games before the ones that merely mention it — and the rest sort
    /// alphabetically, so the order never depends on how the map happened to iterate. A game with
    /// several dumps in the database is offered once: they share a logo.
    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(bool, &str)> = self
            .by_crc
            .values()
            .filter_map(|name| {
                name.to_lowercase()
                    .find(&needle)
                    .map(|at| (at == 0, name.as_str()))
            })
            .collect();
        hits.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        hits.dedup_by(|a, b| a.1 == b.1);
        hits.into_iter()
            .take(limit)
            .map(|(_, name)| name.to_string())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.by_crc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_crc.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXCERPT: &str = include_str!("../tests/fixtures/gba-excerpt.dat");

    #[test]
    fn each_crc_finds_its_game() {
        let dat = Dat::parse(EXCERPT);
        assert_eq!(dat.len(), 3);
        assert_eq!(dat.game_for(0x6C75_479C), Some("Metroid Fusion (USA)"));
        assert_eq!(
            dat.game_for(0x1F1C_08FB),
            Some("Pokemon - Emerald Version (USA, Europe)")
        );
        assert_eq!(
            dat.game_for(0x7570_3943),
            Some("2 Disney Games - Lilo & Stitch 2 + Peter Pan - Return to Neverland (Europe) (En,Fr,De,Es+En,Fr,De,Es,It,Nl)")
        );
    }

    #[test]
    fn search_puts_names_that_start_with_the_query_first() {
        let dat = Dat::parse(EXCERPT);
        // "Pokemon" starts one name and sits inside none of the others, and "2" starts the Disney
        // two-pack while Emerald only contains it.
        assert_eq!(
            dat.search("2", 10),
            vec![
                "2 Disney Games - Lilo & Stitch 2 + Peter Pan - Return to Neverland (Europe) (En,Fr,De,Es+En,Fr,De,Es,It,Nl)",
            ]
        );
    }

    #[test]
    fn search_ignores_case_and_surrounding_space() {
        let dat = Dat::parse(EXCERPT);
        assert_eq!(dat.search("  MeTroid ", 10), vec!["Metroid Fusion (USA)"]);
    }

    #[test]
    fn search_takes_at_most_the_limit_and_nothing_for_an_empty_query() {
        let dat = Dat::parse(EXCERPT);
        assert_eq!(dat.search("e", 2).len(), 2);
        assert!(dat.search("   ", 10).is_empty());
        assert!(dat.search("kirby", 10).is_empty());
    }

    #[test]
    fn a_name_finds_every_dump_it_has_in_order() {
        let two = "game (\n name \"Tetris (World)\"\n rom ( name \"a\" crc 0000000B )\n)\n\
                   game (\n name \"Tetris (World)\"\n rom ( name \"b\" crc 0000000A )\n)\n";
        let dat = Dat::parse(two);
        assert_eq!(dat.crcs_for("Tetris (World)"), vec![0xA, 0xB]);
        assert_eq!(
            Dat::parse(EXCERPT).crcs_for("Metroid Fusion (USA)"),
            vec![0x6C75_479C]
        );
        assert!(dat.crcs_for("Kirby").is_empty());
    }

    #[test]
    fn an_unknown_crc_finds_nothing() {
        assert_eq!(Dat::parse(EXCERPT).game_for(0xDEAD_BEEF), None);
    }

    #[test]
    fn windows_line_endings_parse_the_same() {
        let crlf = EXCERPT.replace('\n', "\r\n");
        assert_eq!(
            Dat::parse(&crlf).game_for(0x6C75_479C),
            Some("Metroid Fusion (USA)")
        );
    }
}

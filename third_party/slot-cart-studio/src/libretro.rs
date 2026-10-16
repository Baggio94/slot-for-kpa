//! libretro-thumbnails' naming, and what raw.githubusercontent.com hands back for a symlink.

/// libretro names a thumbnail after the DAT's game name, with the characters its filesystem
/// rules forbid replaced by `_`.
pub fn thumbnail_name(game: &str) -> String {
    game.chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect()
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// About a third of the logos are git symlinks, which raw.githubusercontent.com serves as the
/// target's file name in plain text. `None` means these bytes are not a stub: the image itself,
/// or a body that is not one plausible file name in the same folder, such as an error page.
pub fn stub_target(body: &[u8]) -> Option<String> {
    if body.starts_with(&PNG_SIGNATURE) || body.len() > 512 {
        return None;
    }
    let name = std::str::from_utf8(body).ok()?.trim();
    (name.ends_with(".png") && !name.contains(['\n', '/'])).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_characters_become_underscores() {
        assert_eq!(
            thumbnail_name("2 Disney Games - Lilo & Stitch 2 + Peter Pan - Return to Neverland (Europe) (En,Fr,De,Es+En,Fr,De,Es,It,Nl)"),
            "2 Disney Games - Lilo _ Stitch 2 + Peter Pan - Return to Neverland (Europe) (En,Fr,De,Es+En,Fr,De,Es,It,Nl)"
        );
        assert_eq!(
            thumbnail_name("a*b/c:d`e<f>g?h\\i|j\"k"),
            "a_b_c_d_e_f_g_h_i_j_k"
        );
        assert_eq!(
            thumbnail_name("Metroid Fusion (USA)"),
            "Metroid Fusion (USA)"
        );
    }

    #[test]
    fn a_png_is_not_a_stub() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(b"the rest of an image");
        assert_eq!(stub_target(&png), None);
    }

    #[test]
    fn a_symlink_body_names_its_target() {
        assert_eq!(
            stub_target(b"Metroid Fusion (Europe) (En,Fr,De,Es,It).png"),
            Some("Metroid Fusion (Europe) (En,Fr,De,Es,It).png".to_string())
        );
        assert_eq!(
            stub_target(b"Drill Dozer (USA).png\n"),
            Some("Drill Dozer (USA).png".to_string())
        );
    }

    #[test]
    fn an_error_page_is_not_followed() {
        assert_eq!(stub_target(b"404: Not Found"), None);
        assert_eq!(
            stub_target(b"<html><body>nope.png</body></html>\n<p>"),
            None
        );
        assert_eq!(stub_target(b"../Named_Boxarts/Drill Dozer (USA).png"), None);
    }
}

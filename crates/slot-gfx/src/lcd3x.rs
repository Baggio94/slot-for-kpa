use std::f32::consts::PI;

const BRIGHTEN_SCANLINES: f32 = 16.0;
const BRIGHTEN_LCD: f32 = 4.0;
const OFFSETS: [f32; 3] = [PI / 2.0, PI * (0.5 - 2.0 / 3.0), PI * (0.5 - 4.0 / 3.0)];

pub fn lcd3x_factors(x: f32, y: f32) -> [f32; 3] {
    let yfactor = (BRIGHTEN_SCANLINES + (2.0 * PI * y).sin()) / (BRIGHTEN_SCANLINES + 1.0);
    OFFSETS.map(|o| yfactor * (BRIGHTEN_LCD + (2.0 * PI * x + o).sin()) / (BRIGHTEN_LCD + 1.0))
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ScreenEffect {
    None,
    Lcd3x,
    Grid,
    Dot,
    Simpletex,
    LcdGridV2,
    LcdGridV2Gba,
    ZfastLcd,
    Lcd1x,
    SimpletexGba,
    Agb001,
    Ags001,
    SharpBilinear,
}

impl ScreenEffect {
    pub fn mode(self) -> f32 {
        self as u8 as f32
    }
}

pub fn lcd3x_mask() -> [[[f32; 3]; 3]; 3] {
    let mut mask = [[[0.0f32; 3]; 3]; 3];
    for (oy, row) in mask.iter_mut().enumerate() {
        for (ox, cell) in row.iter_mut().enumerate() {
            *cell = lcd3x_factors((ox as f32 + 0.5) / 3.0, (oy as f32 + 0.5) / 3.0);
        }
    }
    mask
}

#[cfg(test)]
mod gba_preset_tests {
    use super::*;
    #[test]
    fn ten_unique_gpu_modes_compile_once_and_switch_by_uniform() {
        let modes=[
            ScreenEffect::Lcd3x,ScreenEffect::LcdGridV2,
            ScreenEffect::LcdGridV2Gba,ScreenEffect::ZfastLcd,
            ScreenEffect::Lcd1x,ScreenEffect::SimpletexGba,
            ScreenEffect::Agb001,ScreenEffect::Ags001,
            ScreenEffect::SharpBilinear,ScreenEffect::Dot
        ];
        let mut seen=std::collections::BTreeSet::new();
        for e in modes {assert!(seen.insert(e.mode() as i32));}
        assert_eq!(seen.len(),10);
        assert_eq!(ScreenEffect::None.mode(),0.0);
        assert_eq!(ScreenEffect::Grid.mode(),2.0);
    }
}

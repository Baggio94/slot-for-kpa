use crate::lcd3x::ScreenEffect;
use crate::power::{screen_brightness, screen_rect_in};
use crate::quad::Quad;
use crate::shaders::{GAME_FRAG, RECT_VERT};
use crate::surface::{GfxError, OUT_H, out_w};

pub const SCALE: u32 = 3;
pub const SRC_W: u32 = crate::surface::BASE_W / SCALE;
pub const SRC_H: u32 = OUT_H / SCALE;

pub const WHOLE_TEXTURE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// One game program, specialised for a single screen effect.
struct GameProgram {
    prog: gl::types::GLuint,
    u_rect: gl::types::GLint,
    u_bright: gl::types::GLint,
    u_uv: gl::types::GLint,
    u_colour_correct: gl::types::GLint,
    u_paper_size: gl::types::GLint,
    u_fbo: gl::types::GLint,
    u_pic: gl::types::GLint,
    u_src: gl::types::GLint,
}

impl GameProgram {
    /// The game shader with its effect fixed: the compiler drops every
    /// other effect, so a light effect stays light on a small GPU instead
    /// of paying for the heaviest one's registers.
    fn new(effect: ScreenEffect) -> Result<Self, GfxError> {
        let prog = crate::shaders::program(RECT_VERT, &game_frag(effect))?;
        let at = |name| crate::gl::uniform_location(prog, name);
        unsafe {
            gl::UseProgram(prog);
            gl::Uniform1i(at("u_game"), 0);
            gl::Uniform2f(at("u_src"), SRC_W as f32, SRC_H as f32);
            gl::Uniform2f(at("u_target"), out_w() as f32, OUT_H as f32);
            gl::Uniform1i(at("u_paper"), 1);
        }
        Ok(GameProgram {
            prog,
            u_rect: at("u_rect"),
            u_bright: at("u_bright"),
            u_uv: at("u_uv"),
            u_colour_correct: at("u_colour_correct"),
            u_paper_size: at("u_paper_size"),
            u_fbo: at("u_fbo"),
            u_pic: at("u_pic"),
            u_src: at("u_src"),
        })
    }
}

fn game_frag(effect: ScreenEffect) -> String {
    GAME_FRAG.replace("uniform float u_mode;",
        &format!("const float u_mode = {:.1};", effect.mode()))
}

const EFFECTS: usize = ScreenEffect::CrtAperture as usize + 1;

/// How a home console frame (any size) fills the screen. The Game Boy
/// family keeps its 240x160 canvas and the source/picture rects instead.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Fit {
    /// The 240x160 canvas drawn over the game area.
    Canvas,
    /// Whole-number multiple of the frame height, at the display aspect.
    Integer(f32),
    /// The full height (or width) at the display aspect.
    Aspect(f32),
    /// RetroArch's integer overscale: the next whole multiple up when the
    /// picture doesn't fit exactly, its edges cut. Hi-res frames (448
    /// lines and up) count lines double.
    Overscale(f32),
    /// The whole screen.
    Stretch,
}

/// The texture window that shows a `tex`-sized frame at `fit` on an
/// `out`-sized screen: outside 0..1 is drawn black by the shader.
pub fn fit_window(fit: Fit, tex: (u32, u32), out: (f32, f32)) -> [f32; 4] {
    let (tw, th) = (tex.0.max(1) as f32, tex.1.max(1) as f32);
    let (ow, oh) = out;
    let picture = |aspect: f32| if aspect > 0.0 { aspect } else { tw / th };
    let (pw, ph) = match fit {
        Fit::Canvas | Fit::Stretch => return WHOLE_TEXTURE,
        Fit::Aspect(aspect) => {
            let a = picture(aspect);
            if oh * a <= ow { (oh * a, oh) } else { (ow, ow / a) }
        }
        Fit::Overscale(aspect) => {
            let a = picture(aspect);
            let unit = if th >= 400.0 { th / 2.0 } else { th };
            let k = (oh / unit).min(ow / (unit * a));
            // Within a hair of a whole multiple it fits: no cut.
            let k = if (k - k.round()).abs() < 1e-3 { k.round() } else { k.ceil() }.max(1.0);
            (unit * k * a, unit * k)
        }
        Fit::Integer(aspect) => {
            let a = picture(aspect);
            let mut k = (oh / th).floor().max(1.0);
            while k > 1.0 && th * k * a > ow { k -= 1.0; }
            let ph = th * k;
            ((ph * a).min(ow), ph)
        }
    };
    let (w, h) = (ow / pw, oh / ph);
    [(1.0 - w) / 2.0, (1.0 - h) / 2.0, w, h]
}

pub struct GamePass {
    context_lost: bool,
    /// Compiled on first use, one per effect.
    programs: [Option<GameProgram>; EFFECTS],
    game: gl::types::GLuint,
    // Reuse the Android GLES2-compatible RGBA staging pixels each frame.
    // The old per-frame flat_map().collect() allocated ~150 KiB at 60 Hz.
    #[cfg(target_os = "android")]
    rgba_upload: Vec<u8>,
    colour_correct: bool,
    fbo_scale: [f32; 2],
    pic: [f32; 4],
    paper: gl::types::GLuint,
    paper_size: f32,
    paper_pixels: Option<(u32, Vec<u8>)>,
    effect: ScreenEffect,
    power: f32,
    src: [f32; 4],
    /// Canvas rect the whole 240x160 game texture maps to.
    area: [f32; 4],
    /// The game texture's current size: 240x160, or a home console frame.
    size: (u32, u32),
    fit: Fit,
    /// A home console picture at the top of the screen, not centred (a
    /// Watomsk logo sits in the band under it).
    top: bool,
}

impl GamePass {
    pub(crate) fn abandon_context(&mut self) {
        self.context_lost = true;
    }

    pub(crate) fn recovery_bytes(&self) -> usize {
        self.paper_pixels.as_ref().map_or(0, |p| p.1.len())
    }

    pub(crate) fn restore_settings_into(&self, target: &mut Self) {
        target.colour_correct = self.colour_correct;
        target.fbo_scale = self.fbo_scale;
        target.pic = self.pic;
        target.set_effect(self.effect);
        target.power = self.power;
        target.src = self.src;
        target.area = self.area;
        target.fit = self.fit;
        if let Some((size, rgba)) = &self.paper_pixels {
            target.set_paper(*size, rgba);
        }
    }

    pub fn new() -> Result<Self, GfxError> {
        // OpenGL ES 2 only guarantees RGBA, not BGRA texture uploads.
        #[cfg(target_os = "android")]
        let format = gl::RGBA;
        #[cfg(not(target_os = "android"))]
        let format = gl::BGRA;
        let effect = ScreenEffect::Lcd3x;
        let mut programs: [Option<GameProgram>; EFFECTS] = Default::default();
        programs[effect as usize] = Some(GameProgram::new(effect)?);
        let game = crate::gl::texture(SRC_W, SRC_H, gl::NEAREST, gl::CLAMP_TO_EDGE, format, None);
        let paper = crate::gl::texture(1, 1, gl::NEAREST, gl::REPEAT, gl::RGBA, Some(&[255; 4]));
        Ok(GamePass {
            context_lost: false,
            programs,
            game,
            #[cfg(target_os = "android")]
            rgba_upload: vec![0; (SRC_W * SRC_H * 4) as usize],
            colour_correct: false,
            fbo_scale: [1.0, 1.0],
            pic: WHOLE_TEXTURE,
            paper,
            paper_size: 1.0,
            paper_pixels: None,
            effect,
            power: 1.0,
            src: WHOLE_TEXTURE,
            area: default_area(),
            size: (SRC_W, SRC_H),
            fit: Fit::Canvas,
            top: false,
        })
    }

    pub fn set_power(&mut self, t: f32) {
        self.power = t.clamp(0.0, 1.0);
    }

    pub fn set_picture(&mut self, rect: [f32; 4]) {
        self.pic = rect;
    }

    pub fn set_fbo_scale(&mut self, x: f32, y: f32) {
        self.fbo_scale = [x, y];
    }

    pub fn set_effect(&mut self, effect: ScreenEffect) {
        let slot = &mut self.programs[effect as usize];
        if slot.is_none() {
            match GameProgram::new(effect) {
                Ok(program) => *slot = Some(program),
                // Keep the current effect rather than draw nothing.
                Err(_) => return,
            }
        }
        self.effect = effect;
    }

    pub fn set_colour_correction(&mut self, enabled: bool) {
        self.colour_correct = enabled;
    }

    pub fn set_paper(&mut self, size: u32, rgba: &[u8]) {
        let tex = crate::gl::texture(size, size, gl::NEAREST, gl::REPEAT, gl::RGBA, Some(rgba));
        unsafe { gl::DeleteTextures(1, &self.paper) };
        self.paper = tex;
        self.paper_size = size as f32;
        self.paper_pixels = Some((size, rgba.to_vec()));
    }

    pub fn set_source_rect(&mut self, rect: [f32; 4]) {
        self.src = rect;
    }

    pub fn set_area(&mut self, area: [f32; 4]) {
        self.area = area;
    }

    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = fit;
    }

    pub fn set_top(&mut self, top: bool) {
        self.top = top;
    }

    /// Reallocate the game texture when a frame arrives at another size
    /// (a home console, SNES hi-res, a Mega Drive mode switch).
    fn resize(&mut self, w: u32, h: u32) {
        if self.size == (w, h) {
            return;
        }
        #[cfg(target_os = "android")]
        let format = gl::RGBA;
        #[cfg(not(target_os = "android"))]
        let format = gl::BGRA;
        let tex = crate::gl::texture(w, h, gl::NEAREST, gl::CLAMP_TO_EDGE, format, None);
        unsafe { gl::DeleteTextures(1, &self.game) };
        self.game = tex;
        self.size = (w, h);
        #[cfg(target_os = "android")]
        self.rgba_upload.resize((w * h * 4) as usize, 0);
    }

    pub fn upload(&mut self, xrgb8888: &[u8]) {
        self.upload_sized(xrgb8888, (SRC_W, SRC_H));
    }

    pub fn upload_sized(&mut self, xrgb8888: &[u8], (w, h): (u32, u32)) {
        if w == 0 || h == 0 || xrgb8888.len() < (w * h * 4) as usize {
            return;
        }
        self.resize(w, h);
        #[cfg(target_os = "android")]
        {
            // ES2 requires RGBA; the libretro frame arrives as XRGB8888
            // in little-endian B,G,R,X order. Conversion is still required,
            // but happens in a persistent buffer without heap churn.
            for (src, dst) in xrgb8888
                .chunks_exact(4)
                .zip(self.rgba_upload.chunks_exact_mut(4))
            {
                dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
            }
        }
        #[cfg(target_os = "android")]
        let pixels = self.rgba_upload.as_ptr();
        #[cfg(not(target_os = "android"))]
        let pixels = xrgb8888.as_ptr();
        #[cfg(target_os = "android")]
        let format = gl::RGBA;
        #[cfg(not(target_os = "android"))]
        let format = gl::BGRA;
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.game);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                0,
                0,
                w as i32,
                h as i32,
                format,
                gl::UNSIGNED_BYTE,
                pixels as *const std::ffi::c_void,
            );
        }
    }

    pub fn draw(&self, quad: &Quad) {
        if self.fit != Fit::Canvas {
            // A home console frame fills the whole canvas through its fit window.
            let canvas = [0.0, 0.0, out_w() as f32, OUT_H as f32];
            let out = (canvas[2] * self.fbo_scale[0], canvas[3] * self.fbo_scale[1]);
            let mut window = fit_window(self.fit, self.size, out);
            if self.top { window[1] = 0.0; }
            self.draw_source(self.game, quad, window, self.size, WHOLE_TEXTURE, canvas);
            return;
        }
        let src = match self.effect {
            ScreenEffect::Grid => whole_scale_source(self.src, self.area, self.fbo_scale),
            _ => self.src,
        };
        self.draw_source(self.game, quad, src, self.size, self.pic, self.area);
    }

    pub fn draw_still(&self, tex: gl::types::GLuint, quad: &Quad) {
        self.draw_source(tex, quad, WHOLE_TEXTURE, (SRC_W, SRC_H), self.pic, self.area);
    }

    fn draw_source(&self, tex: gl::types::GLuint, quad: &Quad, src: [f32; 4],
                   size: (u32, u32), pic: [f32; 4], area: [f32; 4]) {
        let Some(p) = &self.programs[self.effect as usize] else { return };
        let (x, y, w, h) = screen_rect_in(area, self.power);
        unsafe {
            gl::UseProgram(p.prog);
            gl::Uniform2f(p.u_src, size.0 as f32, size.1 as f32);
            gl::Uniform4f(p.u_rect, x, y, w, h);
            gl::Uniform4f(p.u_uv, src[0], src[1], src[2], src[3]);
            gl::Uniform1f(p.u_bright, screen_brightness(self.power));
            gl::Uniform1f(
                p.u_colour_correct,
                if self.colour_correct { 1.0 } else { 0.0 },
            );
            gl::Uniform1f(p.u_paper_size, self.paper_size);
            gl::Uniform2f(p.u_fbo, self.fbo_scale[0], self.fbo_scale[1]);
            gl::Uniform4f(p.u_pic, pic[0], pic[1], pic[2], pic[3]);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.paper);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }
        quad.draw();
    }
}

/// The largest 3:2 rect centred on the canvas: the whole canvas on a 3:2
/// panel, full height on a wider one, full width on a narrower one.
pub fn default_area() -> [f32; 4] {
    let (cw, ch) = (out_w() as f32, OUT_H as f32);
    let s = (cw / SRC_W as f32).min(ch / SRC_H as f32);
    let (w, h) = (SRC_W as f32 * s, SRC_H as f32 * s);
    [(cw - w) / 2.0, (ch - h) / 2.0, w, h]
}

fn whole_scale_source(src: [f32; 4], area: [f32; 4], fbo: [f32; 2]) -> [f32; 4] {
    let (x, w) = whole_scale(src[0], src[2], SRC_W as f32, area[2] * fbo[0]);
    let (y, h) = whole_scale(src[1], src[3], SRC_H as f32, area[3] * fbo[1]);
    [x, y, w, h]
}

fn whole_scale(at: f32, len: f32, texels: f32, out: f32) -> (f32, f32) {
    let shown = len * texels;
    let scale = (out / shown).ceil();
    let trim = scale * shown - out;
    if trim >= 0.1 * shown {
        return (at, len);
    }
    let top = (trim / 2.0).floor();
    (at + top / scale / texels, out / scale / texels)
}

impl Drop for GamePass {
    fn drop(&mut self) {
        if self.context_lost {
            return;
        }
        unsafe {
            gl::DeleteTextures(1, &self.game);
            gl::DeleteTextures(1, &self.paper);
            for program in self.programs.iter().flatten() {
                gl::DeleteProgram(program.prog);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_effect_gets_its_own_constant_mode() {
        for effect in [ScreenEffect::None, ScreenEffect::ZfastLcd, ScreenEffect::SharpBilinear] {
            let frag = game_frag(effect);
            assert!(!frag.contains("uniform float u_mode;"));
            assert!(frag.contains(&format!("const float u_mode = {:.1};", effect.mode())));
        }
        assert_eq!(EFFECTS, 19);
    }

    #[test]
    fn original_ratio_pillarboxes_a_4_3_picture_on_3_2() {
        let [x, y, w, h] = fit_window(Fit::Aspect(4.0 / 3.0), (256, 224), (960.0, 640.0));
        assert!((h - 1.0).abs() < 1e-6 && y.abs() < 1e-6);
        // 853.3 of 960 columns show the picture.
        assert!((w - 960.0 / (640.0 * 4.0 / 3.0)).abs() < 1e-5);
        assert!((x + w / 2.0 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn overscale_takes_the_next_whole_multiple_and_cuts_the_edges() {
        // SNES at 8:7 on the Retroid Pocket Classic: x5, 1280x1120.
        let [x, y, w, h] = fit_window(Fit::Overscale(256.0 / 224.0), (256, 224), (1240.0, 1080.0));
        assert!((1080.0 / h - 1120.0).abs() < 1e-2 && (1240.0 / w - 1280.0).abs() < 1e-2);
        assert!(x > 0.0 && y > 0.0, "the window crops the picture");
        // The same picture size for a hi-res interlaced frame.
        let [_, _, w2, h2] = fit_window(Fit::Overscale(256.0 / 224.0), (512, 448), (1240.0, 1080.0));
        assert!((w - w2).abs() < 1e-6 && (h - h2).abs() < 1e-6);
        // NES on a 720x720 square: x3, 768x720.
        let [_, _, w, h] = fit_window(Fit::Overscale(256.0 / 240.0), (256, 240), (720.0, 720.0));
        assert!((720.0 / w - 768.0).abs() < 1e-2 && (720.0 / h - 720.0).abs() < 1e-2);
    }

    #[test]
    fn integer_keeps_whole_line_multiples() {
        // 224 lines on a 640-line panel: twice, 448 lines tall.
        let [_, _, _, h] = fit_window(Fit::Integer(4.0 / 3.0), (256, 224), (960.0, 640.0));
        assert!((640.0 / h - 448.0).abs() < 1e-3);
        // A hi-res interlaced frame keeps the same picture size.
        let [_, _, _, h2] = fit_window(Fit::Integer(4.0 / 3.0), (512, 448), (960.0, 640.0));
        assert!((h - h2).abs() < 1e-6);
    }

    #[test]
    fn stretch_and_canvas_show_the_whole_texture() {
        assert_eq!(fit_window(Fit::Stretch, (320, 224), (960.0, 640.0)), WHOLE_TEXTURE);
        assert_eq!(fit_window(Fit::Canvas, (240, 160), (960.0, 640.0)), WHOLE_TEXTURE);
    }
}

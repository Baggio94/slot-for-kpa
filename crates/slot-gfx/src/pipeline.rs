use crate::lcd3x::ScreenEffect;
use crate::power::{screen_brightness, screen_rect};
use crate::quad::Quad;
use crate::shaders::{GAME_FRAG, RECT_VERT};
use crate::surface::{GfxError, OUT_H, OUT_W};

pub const SCALE: u32 = 3;
pub const SRC_W: u32 = OUT_W / SCALE;
pub const SRC_H: u32 = OUT_H / SCALE;

pub const WHOLE_TEXTURE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// How a home console frame (any size) fills the screen. The Game Boy
/// family keeps its 240x160 canvas and the source/picture rects instead.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Fit {
    /// The 240x160 canvas drawn over the whole screen.
    Canvas,
    /// Whole-number multiple of the frame height, at the display aspect.
    Integer(f32),
    /// The full height (or width) at the display aspect.
    Aspect(f32),
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
    prog: gl::types::GLuint,
    game: gl::types::GLuint,
    // Reuse the Android GLES2-compatible RGBA staging pixels each frame.
    // The old per-frame flat_map().collect() allocated ~150 KiB at 60 Hz.
    #[cfg(target_os = "android")]
    rgba_upload: Vec<u8>,
    u_rect: gl::types::GLint,
    u_bright: gl::types::GLint,
    u_uv: gl::types::GLint,
    u_mode: gl::types::GLint,
    u_colour_correct: gl::types::GLint,
    colour_correct: bool,
    u_paper_size: gl::types::GLint,
    u_fbo: gl::types::GLint,
    fbo_scale: [f32; 2],
    u_pic: gl::types::GLint,
    pic: [f32; 4],
    paper: gl::types::GLuint,
    paper_size: f32,
    paper_pixels: Option<(u32, Vec<u8>)>,
    effect: ScreenEffect,
    power: f32,
    src: [f32; 4],
    u_src: gl::types::GLint,
    /// The game texture's current size: 240x160, or a home console frame.
    size: (u32, u32),
    fit: Fit,
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
        target.effect = self.effect;
        target.power = self.power;
        target.src = self.src;
        target.fit = self.fit;
        if let Some((size, rgba)) = &self.paper_pixels {
            target.set_paper(*size, rgba);
        }
    }

    pub fn new() -> Result<Self, GfxError> {
        let prog = crate::shaders::program(RECT_VERT, GAME_FRAG)?;
        // OpenGL ES 2 only guarantees RGBA, not BGRA texture uploads.
        #[cfg(target_os = "android")]
        let format = gl::RGBA;
        #[cfg(not(target_os = "android"))]
        let format = gl::BGRA;
        let game = crate::gl::texture(SRC_W, SRC_H, gl::NEAREST, gl::CLAMP_TO_EDGE, format, None);
        let paper = crate::gl::texture(1, 1, gl::NEAREST, gl::REPEAT, gl::RGBA, Some(&[255; 4]));
        let (u_rect, u_bright, u_uv, u_mode, u_paper_size, u_pic, u_fbo, u_colour_correct, u_src);
        unsafe {
            gl::UseProgram(prog);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_game"), 0);
            u_src = crate::gl::uniform_location(prog, "u_src");
            gl::Uniform2f(u_src, SRC_W as f32, SRC_H as f32);
            gl::Uniform2f(
                crate::gl::uniform_location(prog, "u_target"),
                OUT_W as f32,
                OUT_H as f32,
            );
            u_rect = crate::gl::uniform_location(prog, "u_rect");
            u_bright = crate::gl::uniform_location(prog, "u_bright");
            u_uv = crate::gl::uniform_location(prog, "u_uv");
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_paper"), 1);
            u_mode = crate::gl::uniform_location(prog, "u_mode");
            u_colour_correct = crate::gl::uniform_location(prog, "u_colour_correct");
            u_paper_size = crate::gl::uniform_location(prog, "u_paper_size");
            u_fbo = crate::gl::uniform_location(prog, "u_fbo");
            u_pic = crate::gl::uniform_location(prog, "u_pic");
        }
        Ok(GamePass {
            context_lost: false,
            prog,
            game,
            #[cfg(target_os = "android")]
            rgba_upload: vec![0; (SRC_W * SRC_H * 4) as usize],
            u_rect,
            u_bright,
            u_uv,
            u_mode,
            u_colour_correct,
            colour_correct: false,
            u_paper_size,
            u_fbo,
            fbo_scale: [1.0, 1.0],
            u_pic,
            pic: WHOLE_TEXTURE,
            paper,
            paper_size: 1.0,
            paper_pixels: None,
            effect: ScreenEffect::Lcd3x,
            power: 1.0,
            src: WHOLE_TEXTURE,
            u_src,
            size: (SRC_W, SRC_H),
            fit: Fit::Canvas,
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

    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = fit;
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
            for (src, dst) in xrgb8888[..(w * h * 4) as usize]
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
            let out = (OUT_W as f32 * self.fbo_scale[0], OUT_H as f32 * self.fbo_scale[1]);
            let window = fit_window(self.fit, self.size, out);
            self.draw_source(self.game, quad, window, self.size, WHOLE_TEXTURE);
            return;
        }
        let src = match self.effect {
            ScreenEffect::Grid => whole_scale_source(self.src, self.fbo_scale),
            _ => self.src,
        };
        self.draw_source(self.game, quad, src, self.size, self.pic);
    }

    pub fn draw_still(&self, tex: gl::types::GLuint, quad: &Quad) {
        self.draw_source(tex, quad, WHOLE_TEXTURE, (SRC_W, SRC_H), self.pic);
    }

    fn draw_source(&self, tex: gl::types::GLuint, quad: &Quad, src: [f32; 4],
                   size: (u32, u32), pic: [f32; 4]) {
        let (x, y, w, h) = screen_rect(self.power);
        unsafe {
            gl::UseProgram(self.prog);
            gl::Uniform2f(self.u_src, size.0 as f32, size.1 as f32);
            gl::Uniform4f(self.u_rect, x, y, w, h);
            gl::Uniform4f(self.u_uv, src[0], src[1], src[2], src[3]);
            gl::Uniform1f(self.u_bright, screen_brightness(self.power));
            gl::Uniform1f(self.u_mode, self.effect.mode());
            gl::Uniform1f(
                self.u_colour_correct,
                if self.colour_correct { 1.0 } else { 0.0 },
            );
            gl::Uniform1f(self.u_paper_size, self.paper_size);
            gl::Uniform2f(self.u_fbo, self.fbo_scale[0], self.fbo_scale[1]);
            gl::Uniform4f(self.u_pic, pic[0], pic[1], pic[2], pic[3]);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.paper);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex);
        }
        quad.draw();
    }
}

fn whole_scale_source(src: [f32; 4], fbo: [f32; 2]) -> [f32; 4] {
    let (x, w) = whole_scale(src[0], src[2], SRC_W as f32, OUT_W as f32 * fbo[0]);
    let (y, h) = whole_scale(src[1], src[3], SRC_H as f32, OUT_H as f32 * fbo[1]);
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
            gl::DeleteProgram(self.prog);
        }
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn original_ratio_pillarboxes_a_4_3_picture_on_3_2() {
        let [x, y, w, h] = fit_window(Fit::Aspect(4.0 / 3.0), (256, 224), (960.0, 640.0));
        assert!((h - 1.0).abs() < 1e-6 && y.abs() < 1e-6);
        // 853.3 of 960 columns show the picture.
        assert!((w - 960.0 / (640.0 * 4.0 / 3.0)).abs() < 1e-5);
        assert!((x + w / 2.0 - 0.5).abs() < 1e-6);
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

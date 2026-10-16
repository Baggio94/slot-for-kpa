use crate::surface::GfxError;

const VERT_PREAMBLE: &str = "#version 330 core\n#define attribute in\n#define varying out\n";

const FRAG_PREAMBLE: &str = "#version 330 core\n#define varying in\n\
                             #define texture2D texture\nout vec4 FRAG_COLOR;\n";

const VERT_PREAMBLE_ES: &str = "";
const FRAG_PREAMBLE_ES: &str = "#define FRAG_COLOR gl_FragColor\n";

pub fn program(vert: &str, frag: &str) -> Result<gl::types::GLuint, GfxError> {
    let (vp, fp) = match crate::gl::es() {
        true => (VERT_PREAMBLE_ES, FRAG_PREAMBLE_ES),
        false => (VERT_PREAMBLE, FRAG_PREAMBLE),
    };
    crate::gl::program(&format!("{vp}{vert}"), &format!("{fp}{frag}"))
}

pub const RECT_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 p = (u_rect.xy + a_pos * u_rect.zw) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

pub const SPRITE_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_rect;
uniform vec2 u_target;
uniform vec2 u_turn;
varying vec2 v_uv;
void main() {
    v_uv = a_pos;
    vec2 mid = u_rect.zw * 0.5;
    vec2 local = a_pos * u_rect.zw - mid;
    vec2 turned = vec2(u_turn.x * local.x - u_turn.y * local.y,
                       u_turn.y * local.x + u_turn.x * local.y);
    vec2 p = (u_rect.xy + a_pos * u_rect.zw + (turned - local)) / u_target;
    gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
}
"#;

pub const GAME_FRAG: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D u_game;
uniform sampler2D u_paper;
uniform vec2 u_src;
uniform vec4 u_uv;
uniform float u_bright;
uniform float u_mode;
uniform float u_colour_correct;
uniform float u_paper_size;
uniform vec4 u_rect;
uniform vec2 u_fbo;
uniform vec4 u_pic;
varying vec2 v_uv;
const vec3 offsets = vec3(3.141592654) * vec3(1.0 / 2.0, 1.0 / 2.0 - 2.0 / 3.0, 1.0 / 2.0 - 4.0 / 3.0);
const vec3 luma_coeff = vec3(0.2126, 0.7152, 0.0722);
float dot_weight(vec2 f, vec2 o, float bloom) {
    // RetroArch Dot defaults: gamma 2.4, shine 0.05, blend 0.65,
    // soft 0.0. With soft 0.0, all nine taps share one source sample.
    return exp(-2.4 * length(f - (o + vec2(0.5))) * bloom);
}

/* GLES2 ports of libretro/glsl-shaders (see licenses/shaders.txt).
   No recompilation on hot switching: all modes are linked once at startup.
   Multi-pass AGB/AGS and textured presets are analytic/native-scale ports
   requiring visual QA before claiming byte-for-byte RetroArch parity. */
float lcd_integral(float z, bool y) {
    float a = y ? 0.0 : -2.0/3.0;
    float b = y ? -4.0/5.0 : -1.0/5.0;
    float d = y ? 2.0/7.0 : 4.0/7.0;
    float e = y ? 4.0/9.0 : -1.0/9.0;
    float g = y ? -4.0/11.0 : -2.0/11.0;
    float z2 = z*z;
    return z*(1.0+z2*(a+z2*(b+z2*(d+z2*(e+z2*(g+z2/13.0))))));
}
float lcd_smear(float x, float dx, float radius, bool y) {
    dx = max(dx, 0.0001);
    float lo = clamp((x-dx*0.5)/radius, -1.0, 1.0);
    float hi = clamp((x+dx*0.5)/radius, -1.0, 1.0);
    return radius*(lcd_integral(hi,y)-lcd_integral(lo,y))/dx;
}
vec3 gba_color(vec3 c) {
    // hunterk/Pokefan531 official gba-color default matrix and gamma.
    c = pow(max(c,vec3(0.0)),vec3(3.2));
    c = mat3(0.82,0.125,0.195, 0.24,0.665,0.075, -0.06,0.21,0.73)*(c*0.94);
    return pow(clamp(c,0.0,1.0),vec3(1.0/2.2));
}
vec3 gba_simpletex_color(vec3 c) {
    c = pow(max(c,vec3(0.0)),vec3(3.25));
    c = mat3(0.82,0.125,0.195, 0.24,0.665,0.075, -0.06,0.21,0.73)*(c*0.94);
    return clamp(pow(max(c,vec3(0.0)),vec3(1.0/2.2)),0.0,1.0);
}
vec3 lcd_grid_v2(vec2 uv, bool bgr) {
    // cgwg LCD Grid V2 coefficient integration, rewritten without GLSL130
    // array constructors (unsupported on GLES2). Same official preset
    // parameters: gain=1.5, gamma=2.2, RGB matrix diagonal=0.75.
    vec2 xy = uv*u_src;
    vec2 tli = floor(xy-vec2(0.4999));
    vec2 range = vec2(1.0)/max(u_rect.zw*u_fbo,vec2(1.0));
    float sub = (xy.x-0.4999-tli.x)*3.0;
    float dx = range.x*3.0;
    vec3 left = vec3(lcd_smear(sub+1.0,dx,1.5,false),
                     lcd_smear(sub,dx,1.5,false),
                     lcd_smear(sub-1.0,dx,1.5,false));
    vec3 right = vec3(lcd_smear(sub-2.0,dx,1.5,false),
                      lcd_smear(sub-3.0,dx,1.5,false),
                      lcd_smear(sub-4.0,dx,1.5,false));
    if (bgr) { left=left.bgr; right=right.bgr; }
    float py = xy.y-0.4999-tli.y;
    float top = lcd_smear(py,range.y,0.63,true);
    float bot = lcd_smear(py-1.0,range.y,0.63,true);
    vec2 pixel = vec2(1.0)/u_src;
    vec2 pos = (tli+vec2(0.5))*pixel;
    vec3 tl=pow(1.5*texture2D(u_game,pos).rgb,vec3(2.2));
    vec3 tr=pow(1.5*texture2D(u_game,pos+vec2(pixel.x,0.0)).rgb,vec3(2.2));
    vec3 bl=pow(1.5*texture2D(u_game,pos+vec2(0.0,pixel.y)).rgb,vec3(2.2));
    vec3 br=pow(1.5*texture2D(u_game,pos+pixel).rgb,vec3(2.2));
    vec3 weighted=(tl*left+tr*right)*top+(bl*left+br*right)*bot;
    return pow(max(weighted*0.75,vec3(0.0)),vec3(1.0/2.2));
}
vec3 mgba_mask(vec2 uv) {
    // endrift AGB/AGS-001 original 4x RGB subpixel mask.
    float px=mod((uv*u_src).x*4.0,4.0);
    if(px<1.0) return vec3(1.0,0.2,0.2);
    if(px<2.0) return vec3(0.2,1.0,0.2);
    if(px<3.0) return vec3(0.2,0.2,1.0);
    return vec3(0.4);
}
vec3 ags_first_pass(vec2 uv) {
    float py=mod((uv*u_src).y*4.0,4.0);
    return pow(texture2D(u_game,uv).rgb,vec3(1.6))*mgba_mask(uv)*
           (py>=3.0 ? 0.9 : 1.0);
}
vec3 nearest_bilinear(vec2 uv) {
    // GPU sampler remains NEAREST for the other presets. Manually emulate
    // the RetroArch sharp-bilinear LINEAR filter using four source texels.
    vec2 coord=uv*u_src-vec2(0.5);
    vec2 base=floor(coord);
    vec2 t=fract(coord);
    vec2 stepuv=vec2(1.0)/u_src;
    vec2 pos=(base+vec2(0.5))*stepuv;
    vec3 a=texture2D(u_game,pos).rgb;
    vec3 b=texture2D(u_game,pos+vec2(stepuv.x,0.0)).rgb;
    vec3 c=texture2D(u_game,pos+vec2(0.0,stepuv.y)).rgb;
    vec3 d=texture2D(u_game,pos+stepuv).rgb;
    return mix(mix(a,b,t.x),mix(c,d,t.x),t.y);
}
void main() {
    vec2 uv = u_uv.xy + v_uv * u_uv.zw;
    vec2 f = fract(uv * u_src);
    vec3 c = texture2D(u_game, uv).rgb;
    bool inside = all(greaterThanEqual(uv, u_pic.xy)) && all(lessThan(uv, u_pic.zw));

    if (!inside) {
    } else if (u_mode > 11.5) {
        // Themaister sharp-bilinear: automatic prescale=3 at Slot 720x480.
        vec2 texel=uv*u_src;
        vec2 s=fract(texel);
        float scale=max(1.0,floor((u_rect.w*u_fbo.y)/u_src.y+0.01));
        float bound=0.5-0.5/scale;
        vec2 delta=s-vec2(0.5);
        vec2 filtered=(delta-clamp(delta,vec2(-bound),vec2(bound)))*scale+0.5;
        c=nearest_bilinear((floor(texel)+filtered)/u_src);
    } else if (u_mode > 10.5) {
        // endrift AGS-001: frontlit SP gamma, subpixel mask and light.
        c=ags_first_pass(uv);
        vec2 radius=pow(abs((v_uv-vec2(0.5))*1.5),vec2(4.0));
        vec3 bleed=vec3(0.12,0.14,0.19) +
            (dot(radius,radius)+vec3(0.02,0.03,0.05))*vec3(0.14,0.18,0.2);
        c+=bleed+0.07*ags_first_pass(uv-vec2(0.0,0.025));
    } else if (u_mode > 9.5) {
        // endrift AGB-001: original non-backlit screen color subpixels.
        float py=mod((uv*u_src).y*4.0,4.0);
        c=(pow(c*0.8,vec3(1.8))+vec3(0.16))*mgba_mask(uv)*
            (py>=3.0 ? 0.8 : 1.0);
    } else if (u_mode > 8.5) {
        // jdgleaver simpletex_lcd+gba-color: official grid parameters,
        // using Slot's existing licensed paper texture crop.
        c=gba_simpletex_color(c);
        vec2 dd=abs(f-vec2(0.5));
        float d2=max(dd.x,dd.y); d2*=d2;
        float d4=d2*d2;
        float weight=48.0*(d4-(8.0/3.0)*d4*d2);
        weight=weight*(weight+(1.0-weight)*0.65)*0.65;
        float lum=dot(c,luma_coeff);
        weight*=lum+(1.0-lum)*0.4;
        c=mix(c,vec3(1.0),weight);
        vec3 paper=texture2D(u_paper,
            (floor(gl_FragCoord.xy)+vec2(0.5))/u_paper_size).rgb;
        c=mix(c,paper*c,dot(c,luma_coeff));
    } else if (u_mode > 7.5) {
        // jdgleaver LCD1x: proper scanline alignment, no RGB separation.
        vec2 angle=6.283185307*(uv*u_src-vec2(0.25));
        c*=((16.0+sin(angle.y))/17.0)*((4.0+sin(angle.x))/5.0);
    } else if (u_mode > 6.5) {
        // SoltanGris42 zfast_lcd_standard default BORDERMULT=14,
        // GBAGAMMA=1, nearest sampling.
        vec2 px=uv*u_src;
        vec2 dist=abs(floor(px)+vec2(0.5)-px);
        float y=max(dist.x,dist.y); y*=y;
        float yy=y*y;
        float weight=1.0-14.0*(yy-2.7*yy*y);
        c*=weight;
        c*=0.6+0.4*c;
    } else if (u_mode > 5.5) {
        // CGWG LCD Grid V2 followed by RetroArch GBA Color.
        c=gba_color(lcd_grid_v2(uv,true));
    } else if (u_mode > 4.5) {
        c=lcd_grid_v2(uv,false);
    } else if (u_mode > 3.5) {
        c = pow(c, vec3(1.05));
        vec2 d = abs(f - 0.5);
        float x2 = max(d.x, d.y);
        x2 = x2 * x2;
        float x4 = x2 * x2;
        float w = 48.0 * (x4 - (8.0 / 3.0) * x4 * x2);
        w = w * (w + (1.0 - w) * 0.65) * 0.65;
        float luma = dot(c, luma_coeff);
        w = w * luma;
        c = mix(c, vec3(1.0), w);
        vec3 bg = texture2D(u_paper, (floor(gl_FragCoord.xy) + 0.5) / u_paper_size).rgb * c;
        c = mix(c, bg, dot(c, luma_coeff));
    } else if (u_mode > 2.5) {
        float bloom = mix(1.05, 0.95, dot(c, vec3(0.30, 0.59, 0.11)));
        float mid = dot_weight(f, vec2(0.0), bloom);
        float sum = 0.0;
        for (int j = -1; j <= 1; j++) {
            for (int i = -1; i <= 1; i++) {
                sum += dot_weight(f, vec2(float(i), float(j)), bloom);
            }
        }
        c = mix(1.2 * c * mid, c * sum, 0.65);
    } else if (u_mode > 1.5) {
        vec2 hp = 0.5 * u_uv.zw * u_src / (u_rect.zw * u_fbo);
        vec2 edge = max(step(f - hp, vec2(1e-4)), step(vec2(1.0) + 1e-4, f + hp));
        c *= 1.0 - 0.75 * max(edge.x, edge.y);
    } else if (u_mode > 0.5) {
        vec2 angle = f * 6.283185307;
        float yfactor = (16.0 + sin(angle.y)) / 17.0;
        vec3 xfactors = (4.0 + sin(angle.x + offsets)) / 5.0;
        c *= yfactor * xfactors;
    }
    // These two RetroArch presets already contain GBA Color. Never
    // unintentionally apply the global correction twice.
    if (u_colour_correct > 0.5 && inside &&
        !(u_mode > 5.5 && u_mode < 6.5) &&
        !(u_mode > 8.5 && u_mode < 9.5)) {
        // GBA-era display simulation: subtly warmer, lower-saturation
        // colours and a restrained luminance response. GPU only; libretro
        // framebuffer, SRAM and save states remain untouched.
        float luminance = dot(c, vec3(0.299, 0.587, 0.114));
        c = mix(vec3(luminance), c, 0.80);
        c *= vec3(1.02, 1.00, 0.95);
        c = pow(clamp(c, 0.0, 1.0), vec3(1.05));
    }
    FRAG_COLOR = vec4(c * u_bright, 1.0);
}
"#;

pub const SPRITE_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec4 u_colour;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = texture2D(u_tex, v_uv) * u_colour;
}
"#;

pub const BLIT_VERT: &str = r#"
attribute vec2 a_pos;
uniform vec4 u_uv;
varying vec2 v_uv;
void main() {
    v_uv = u_uv.xy + a_pos * u_uv.zw;
    gl_Position = vec4(a_pos * 2.0 - 1.0, 0.0, 1.0);
}
"#;

pub const BLIT_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_gain;
varying vec2 v_uv;
void main() {
    FRAG_COLOR = vec4(texture2D(u_tex, v_uv).rgb * u_gain, 1.0);
}
"#;

pub const BEZEL_FRAG: &str = r#"
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_gain;
uniform float u_alpha;
varying vec2 v_uv;
void main() {
    vec4 c = texture2D(u_tex, v_uv);
    FRAG_COLOR = vec4(c.rgb * u_gain, c.a * u_alpha);
}
"#;

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
    // Game texels per screen pixel, as upstream's range/texelSize: each
    // screen pixel integrates the subpixel profile over its own width.
    vec2 range = (u_uv.zw*u_src)/(u_rect.zw*u_fbo);
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
/* endrift AGB/AGS-001 are two passes upstream: the game at 4x, nearest,
   through the 4x4 subpixel mask, then that picture scaled to the screen
   with LINEAR filtering. sub_4x is one texel of the 4x picture and
   mgba_pass filters four of them, so the mask keeps even columns and rows
   at any scale (at exactly 4x it is the first pass as it is). */
vec3 mgba_sub_4x(vec2 i, bool ags) {
    vec2 m = mod(i, 4.0);
    vec3 c = texture2D(u_game, (floor(i/4.0) + vec2(0.5))/u_src).rgb;
    c = ags ? pow(c, vec3(1.6)) : pow(c*0.8, vec3(1.8)) + vec3(0.16);
    vec3 mask = m.x < 0.5 ? vec3(1.0,0.2,0.2)
        : m.x < 1.5 ? vec3(0.2,1.0,0.2)
        : m.x < 2.5 ? vec3(0.2,0.2,1.0) : vec3(0.4);
    return c*mask*(m.y > 2.5 ? (ags ? 0.9 : 0.8) : 1.0);
}
vec3 mgba_pass(vec2 uv, bool ags) {
    vec2 q = uv*u_src*4.0 - vec2(0.5);
    vec2 b = floor(q);
    vec2 t = fract(q);
    return mix(mix(mgba_sub_4x(b, ags), mgba_sub_4x(b + vec2(1.0,0.0), ags), t.x),
               mix(mgba_sub_4x(b + vec2(0.0,1.0), ags), mgba_sub_4x(b + vec2(1.0), ags), t.x), t.y);
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
/* Pixel AA (after fishku's pixel_aa / shimmerless): at a non-integer
   scale every game pixel keeps one width and only the screen pixel that
   straddles two of them blends them, by area, in linear light. At a whole
   scale it is exactly the nearest texel. No pass and no name of its own:
   the LCD shaders read the game through it (Siham 2026-11-04). */
vec3 aa_tap(vec2 at) {
    return pow(texture2D(u_game, at).rgb, vec3(2.2));
}
vec3 pixel_aa(vec2 uv) {
    vec2 s = (u_rect.zw*u_fbo)/(u_uv.zw*u_src);
    vec2 texel = uv*u_src - vec2(0.5);
    vec2 base = floor(texel);
    vec2 t = clamp((fract(texel) - vec2(0.5))*s + vec2(0.5), 0.0, 1.0);
    vec2 step1 = vec2(1.0)/u_src;
    vec2 at = (base + vec2(0.5))*step1;
    /* Same picture, less work: most screen pixels lie inside one game
       pixel (t is 0 or 1 on both axes) and are that texel as it is; one
       straddling a single edge blends two texels, a corner all four. */
    vec2 edge = step(vec2(0.001), t) * step(t, vec2(0.999));
    if (edge.x + edge.y < 0.5) {
        return texture2D(u_game, at + step1*step(vec2(0.5), t)).rgb;
    }
    if (edge.y < 0.5) {
        vec2 row = at + vec2(0.0, step1.y*step(0.5, t.y));
        return pow(mix(aa_tap(row), aa_tap(row + vec2(step1.x, 0.0)), t.x), vec3(1.0/2.2));
    }
    if (edge.x < 0.5) {
        vec2 column = at + vec2(step1.x*step(0.5, t.x), 0.0);
        return pow(mix(aa_tap(column), aa_tap(column + vec2(0.0, step1.y)), t.y), vec3(1.0/2.2));
    }
    vec3 a = aa_tap(at);
    vec3 b = aa_tap(at + vec2(step1.x, 0.0));
    vec3 c = aa_tap(at + vec2(0.0, step1.y));
    vec3 d = aa_tap(at + step1);
    return pow(mix(mix(a, b, t.x), mix(c, d, t.x), t.y), vec3(1.0/2.2));
}
/* hunterk's fakelottes: crt-lottes' shadow mask and gamma with a sine
   scanline, in one cheap pass. Its defaults (mask 1, maskDark 0.5,
   maskLight 1.5, CRT gamma 2.5, monitor gamma 2.2, scanline 0.95 + 0.40
   sine), bilinear input as its preset; without its curvature, as CRT
   Geom Deluxe here. */
vec3 fakelottes_mask(vec2 pos) {
    vec3 mask = vec3(0.5);
    float line = 1.5;
    float odd = 0.0;
    if (fract(pos.x*0.166666666) < 0.5) odd = 1.0;
    if (fract((pos.y + odd)*0.5) < 0.5) line = 0.5;
    pos.x = fract(pos.x*0.333333333);
    if (pos.x < 0.333) mask.r = 1.5;
    else if (pos.x < 0.666) mask.g = 1.5;
    else mask.b = 1.5;
    return mask*line;
}
vec3 fakelottes(vec2 uv) {
    vec3 res = pow(nearest_bilinear(uv), vec3(2.5));
    res *= fakelottes_mask(gl_FragCoord.xy*1.0001);
    // Its sine has no horizontal part at the defaults (SINE_COMP_A 0).
    float sine = sin((uv.y - 0.25/u_src.y)*6.283*u_src.y);
    res *= 0.95 + 0.40*sine;
    return pow(max(res, vec3(0.0)), vec3(1.0/2.2));
}
/* EasyMode's crt-aperture (GPL), already one pass: Gaussian glow, two
   Lanczos sharpness taps, beam-shaped scanlines and an aperture-grille
   mask, at its defaults. */
vec3 aperture_tap(vec2 at) {
    // pow(texel, 2.4) as GLSL defines pow, exp2(y*log2(x)): Mesa's llvmpipe
    // miscompiles a pow straight on a texture fetch in this program.
    vec3 t = texture2D(u_game, at).rgb;
    return exp2(log2(max(t, vec3(1e-5)))*2.4);
}
vec3 aperture_blur(vec3 a, vec3 b, vec3 c, float dist, float rad) {
    vec3 x = vec3(dist - 1.0, dist, dist + 1.0)/rad;
    vec3 w = exp2(x*x*-1.0);
    return (a*w.x + b*w.y + c*w.z)/(w.x + w.y + w.z);
}
vec3 aperture_gaussian(vec2 co) {
    vec2 dx = vec2(1.0/u_src.x, 0.0);
    vec2 dy = vec2(0.0, 1.0/u_src.y);
    vec2 pix = co*u_src;
    vec2 tc = (floor(pix) + 0.5)/u_src;
    vec2 dist = (fract(pix) - 0.5)*-1.0;
    vec3 l0 = aperture_blur(aperture_tap(tc-dy-dx), aperture_tap(tc-dy), aperture_tap(tc-dy+dx), dist.x, 0.5);
    vec3 l1 = aperture_blur(aperture_tap(tc-dx), aperture_tap(tc), aperture_tap(tc+dx), dist.x, 0.5);
    vec3 l2 = aperture_blur(aperture_tap(tc+dy-dx), aperture_tap(tc+dy), aperture_tap(tc+dy+dx), dist.x, 0.5);
    return aperture_blur(l0, l1, l2, dist.y, 0.5);
}
vec3 aperture_lanczos(vec2 co, float sharp) {
    vec2 size = vec2(u_src.x*sharp, u_src.y);
    vec2 dx = vec2(1.0/size.x, 0.0);
    vec2 pix = co*size - vec2(0.5, 0.0);
    vec2 tc = (floor(pix) + vec2(0.5, 0.0))/size;
    float d = fract(pix.x);
    vec4 coef = 3.141592653589*vec4(d + 1.0, d, d - 1.0, d - 2.0);
    coef = max(abs(coef), vec4(1e-5));
    coef = 2.0*sin(coef)*sin(coef/2.0)/(coef*coef);
    coef /= dot(coef, vec4(1.0));
    vec3 c1 = aperture_tap(tc);
    vec3 c2 = aperture_tap(tc + dx);
    return c1*(coef.x + coef.y) + c2*(coef.z + coef.w);
}
vec3 crt_aperture(vec2 uv) {
    // Screen pixels per source row (crt_out_px().y, defined further down).
    float scale = floor((u_rect.w*u_fbo.y)/(u_uv.w*u_src.y) + 0.001);
    float offset = 1.0/scale*0.5;
    if (mod(scale, 2.0) > 0.5) offset = 0.0;
    vec2 co = (uv*u_src - vec2(0.0, offset))/u_src;
    vec3 glow = aperture_gaussian(co);
    vec3 soft = aperture_lanczos(co, 1.0);
    vec3 sharp = aperture_lanczos(co, 3.0);
    vec3 col = sqrt(sharp*soft);
    // Scanline weight, beam from 0.5 to 1.5 rows, shape 1.5.
    vec3 beam = mix(vec3(0.5), vec3(1.5), pow(soft, vec3(1.0/1.5)));
    vec3 xm = 2.0/beam;
    vec3 xo = xm*0.5;
    col *= smoothstep(0.0, 1.0, 1.0 - abs(fract(co.y*u_src.y)*xm - xo))*xo;
    glow = clamp(glow - col, 0.0, 1.0);
    col += glow*glow*0.1;
    // Two-colour aperture grille, one screen pixel per stripe, strength 0.3.
    vec3 mask = mod(floor(gl_FragCoord.x), 2.0) < 0.5 ? vec3(1.0, 0.0, 1.0) : vec3(0.0, 1.0, 0.0);
    col = mix(col, col*mask*2.0, 0.3);
    col += glow*0.05;
    return pow(max(col*1.5, vec3(0.0)), vec3(1.0/2.4));
}
/* CRT presets for the home consoles (libretro glsl/slang-shaders, GPL; see
   licenses/shaders.txt). One pass each: slot. draws the game in a single
   pass, so multi-pass parts are folded in or approximated as noted. */
vec2 crt_out_px() {
    // Screen pixels per source texel, per axis.
    return (u_rect.zw*u_fbo)/(u_uv.zw*u_src);
}
float crtpi_weight(float dist) {
    return max(1.0-dist*dist*6.0, 0.12);
}
vec3 gdv_mask(vec2 pos) {
    // Mask 11 (the Trinitron preset's), maskDark 0.0.
    pos = floor(pos);
    return fract(pos.x/3.0) > 0.333 ? vec3(1.0) : vec3(0.0);
}
float gdv_sw(float x, float l) {
    return exp2(-10.0*pow(max(x,0.0), mix(1.5,2.0,l)));
}
vec3 gdv_tap(vec2 at) {
    return texture2D(u_game, at).rgb;
}
vec3 gdv_glow(vec2 tc) {
    // Upstream squares nothing here (its "c*c;" lines are no-ops).
    vec2 b = vec2(1.0)/u_src;
    vec3 c11 = gdv_tap(tc);
    vec3 ring1 = gdv_tap(tc+vec2(-b.x,0.0)) + gdv_tap(tc+vec2(b.x,0.0))
        + gdv_tap(tc+vec2(-b.x,-b.y)) + gdv_tap(tc+vec2(b.x,b.y))
        + gdv_tap(tc+vec2(-b.x,b.y)) + gdv_tap(tc+vec2(b.x,-b.y))
        + gdv_tap(tc+vec2(0.0,-b.y)) + gdv_tap(tc+vec2(0.0,b.y));
    vec3 ring2 = gdv_tap(tc+vec2(-2.0*b.x,0.0)) + gdv_tap(tc+vec2(2.0*b.x,0.0))
        + gdv_tap(tc+vec2(-2.0*b.x,-b.y)) + gdv_tap(tc+vec2(-b.x,-2.0*b.y))
        + gdv_tap(tc+vec2(b.x,2.0*b.y)) + gdv_tap(tc+vec2(2.0*b.x,b.y))
        + gdv_tap(tc+vec2(-2.0*b.x,b.y)) + gdv_tap(tc+vec2(-b.x,2.0*b.y))
        + gdv_tap(tc+vec2(b.x,-2.0*b.y)) + gdv_tap(tc+vec2(2.0*b.x,-b.y))
        + gdv_tap(tc+vec2(0.0,-2.0*b.y)) + gdv_tap(tc+vec2(0.0,2.0*b.y));
    return (3.0*c11 + 2.5*ring1 + 1.5*ring2)/45.0*0.35;
}
vec3 gdv_saturation(vec3 c) {
    float l = length(c)*0.5775;
    vec3 w = vec3(0.3,0.6,0.1);
    if (l < 0.5) w = w*w + w*w;
    return mix(vec3(dot(c,w)), c, 1.2);
}
vec3 crt_gdv_mini_ultra(vec2 uv) {
    // guest(r) + Dr. Venom crt-gdv-mini-ultra, Trinitron preset defaults,
    // CurvatureX/Y 0 (Siham 2026-11-04).
    vec2 ps = vec2(1.0)/u_src;
    vec2 at = uv*u_src;
    vec2 fp = fract(at);
    vec2 pc = floor(at)*ps + 0.5*ps;
    vec3 ul = gdv_tap(pc);
    vec3 ur = gdv_tap(pc+vec2(ps.x,0.0));
    vec3 dl = gdv_tap(pc+vec2(0.0,ps.y));
    vec3 dr = gdv_tap(pc+ps);
    float lx = pow(fp.x, 2.5);
    float rx = pow(1.0-fp.x, 2.5);
    vec3 c1 = (ur*lx + ul*rx)/(lx+rx);
    vec3 c2 = (dr*lx + dl*rx)/(lx+rx);
    float l1 = dot(c1, vec3(0.3,0.6,0.1));
    float l2 = dot(c2, vec3(0.3,0.6,0.1));
    c1 = 2.0*pow(c1,vec3(2.8)) - pow(c1,vec3(3.6));
    c2 = 2.0*pow(c2,vec3(2.8)) - pow(c2,vec3(3.6));
    vec3 mask = gdv_mask(gl_FragCoord.xy);
    c1 *= mix(mask, vec3(1.0), l1*0.4);
    c2 *= mix(mask, vec3(1.0), l2*0.4);
    vec3 c = c1*gdv_sw(fp.y,l1) + c2*gdv_sw(1.0-fp.y,l2);
    if (u_src.y >= 400.0) c = (c1+c2)/2.0;
    c = min(c, 1.0);
    float lum = dot(c, vec3(0.3,0.6,0.1));
    c = pow(max(c,vec3(0.0)), vec3(1.0/2.2));
    c += gdv_glow(pc);
    c *= mix(1.0, 1.2, lum);
    c = gdv_saturation(c);
    vec2 v = uv*(1.0-uv);
    c *= min(pow(max(v.x*v.y*45.0,0.0), 0.15), 1.0);
    return c;
}
vec4 geom_tex(vec2 at) {
    vec2 under = step(0.0,at)*step(0.0,vec2(1.0)-at);
    return pow(texture2D(u_game,at), vec4(2.4))*(under.x*under.y);
}
vec4 geom_weights(float dist, vec4 col) {
    vec4 wid = 2.0 + 2.0*pow(col, vec4(4.0));
    vec4 w = vec4(dist/0.3);
    return 1.4*exp(-pow(w*inversesqrt(0.5*wid), wid))/(0.6+0.2*wid);
}
vec3 geom_blur(vec2 at) {
    // Halation source: upstream blurs the frame in two extra passes; here a
    // gaussian of the same width from four bilinear taps and the centre.
    float wid = 2.0*u_src.x/320.0;
    vec2 o = vec2(0.6*wid)/u_src;
    vec3 b = 0.2*texture2D(u_game, at).rgb
        + 0.2*nearest_bilinear(at+vec2(-o.x,-o.y))
        + 0.2*nearest_bilinear(at+vec2(o.x,-o.y))
        + 0.2*nearest_bilinear(at+vec2(-o.x,o.y))
        + 0.2*nearest_bilinear(at+vec2(o.x,o.y));
    vec3 col = pow(b, vec3(2.4));
    vec2 c = min(at, vec2(1.0)-at)*vec2(1.0,0.75)/(2.0/320.0);
    vec2 e2c = exp(-c*c);
    c = (step(0.0,c)-vec2(0.5))*sqrt(vec2(1.0)-e2c)*(vec2(1.0)+vec2(0.1749)*e2c)+vec2(0.5);
    return col*c.x*c.y;
}
vec3 crt_geom_deluxe(vec2 xy) {
    // cgwg/DOLLS crt-geom-deluxe, curvature 0 (Siham 2026-11-04). Phosphor
    // persistence and raster bloom (two passes and a mipmap) are left out.
    vec2 cxy = min(xy, vec2(1.0)-xy)*vec2(1.0,0.75);
    cxy = vec2(0.01) - min(cxy, vec2(0.01));
    float cval = clamp((0.01-length(cxy))*1000.0, 0.0, 1.0);
    vec2 il = vec2(1.0, clamp(floor(u_src.y/200.0), 1.0, 2.0));
    vec2 one = il/u_src;
    vec2 rs = (xy*u_src - vec2(0.5))/il;
    float over = (1.0/crt_out_px().y)/il.y;
    vec2 ur = fract(rs);
    vec2 at = (floor(rs)*il + vec2(0.5))/u_src;
    vec4 k = 3.141592653589*vec4(1.0+ur.x, ur.x, 1.0-ur.x, 2.0-ur.x);
    k = max(abs(k), 1e-5);
    k = 2.0*sin(k)*sin(k/2.0)/(k*k);
    k /= dot(k, vec4(1.0));
    vec4 col = clamp(geom_tex(at+vec2(-one.x,0.0))*k.x + geom_tex(at)*k.y
        + geom_tex(at+vec2(one.x,0.0))*k.z + geom_tex(at+vec2(2.0*one.x,0.0))*k.w, 0.0, 1.0);
    vec4 col2 = clamp(geom_tex(at+vec2(-one.x,one.y))*k.x + geom_tex(at+vec2(0.0,one.y))*k.y
        + geom_tex(at+one)*k.z + geom_tex(at+vec2(2.0*one.x,one.y))*k.w, 0.0, 1.0);
    // OVERSAMPLE: three samples a third of a screen row apart.
    vec4 w = geom_weights(ur.y, col);
    vec4 w2 = geom_weights(1.0-ur.y, col2);
    float y = ur.y + over/3.0;
    w = (w + geom_weights(y, col))/3.0;
    w2 = (w2 + geom_weights(abs(1.0-y), col2))/3.0;
    y -= 2.0*over/3.0;
    w += geom_weights(abs(y), col)/3.0;
    w2 += geom_weights(abs(1.0-y), col2)/3.0;
    vec3 m = (col*w + col2*w2).rgb;
    vec3 blur = geom_blur(xy);
    m = mix(m, blur, 0.1)*cval;
    m = mix(m, blur, 0.1)*cval;
    // Aperture mask 1 (magenta/green), strength 0.4, brightboost 0.4.
    vec3 mask = mod(floor(gl_FragCoord.x), 2.0) < 0.5 ? vec3(1.0,0.0,1.0) : vec3(0.0,1.0,0.0);
    vec2 ts = crt_out_px();
    float fbright = 127.5/(ts.x*ts.y);
    float avg = mix(1.0-0.4*(1.0-0.4), 1.0, fbright);
    vec3 clow = vec3(0.6)*m + vec3(0.16)*m*m;
    float ifb = 1.0/fbright;
    vec3 chi = vec3(ifb*avg)*m - vec3(ifb-1.0)*clow;
    return pow(max(mix(clow, chi, mask), vec3(0.0)), vec3(1.0/2.2));
}
/* Grid as slot. original draws it: a dark band over the first third of
   each game pixel, left and top, darkening a screen pixel by the share of
   it the band covers. The band grows with the picture, so every line keeps
   a solid core at any scale; the colours come through Pixel AA. */
vec2 grid_band(vec2 x) {
    return floor(x)/3.0 + min(fract(x), vec2(1.0/3.0));
}
vec3 grid_line(vec2 uv, vec3 c) {
    vec2 hp = 0.5*(u_uv.zw*u_src)/(u_rect.zw*u_fbo);
    vec2 at = uv*u_src;
    vec2 edge = (grid_band(at + hp) - grid_band(at - hp))/(2.0*hp);
    return c*(1.0 - 0.75*max(edge.x, edge.y));
}
void main() {
    vec2 uv = u_uv.xy + v_uv * u_uv.zw;
    vec2 f = fract(uv * u_src);
    vec3 c;
    // The LCD shaders (LCD3x, Grid, Dot, Simpletex, zfast LCD, LCD1x,
    // Simpletex + GBA) read the game through Pixel AA. LCD Grid V2 and
    // AGB/AGS-001 filter the game themselves; the others as before.
    if (u_mode > 0.5 && u_mode < 9.5 && !(u_mode > 4.5 && u_mode < 6.5)) {
        c = pixel_aa(uv);
    } else {
        c = texture2D(u_game, uv).rgb;
    }
    bool inside = all(greaterThanEqual(uv, u_pic.xy)) && all(lessThan(uv, u_pic.zw));

    if (!inside) {
        // Outside the picture (a home console's side bands at Integer or
        // Original Ratio): black, as RetroArch, never the edge texel smeared.
        c = vec3(0.0);
    } else if (u_mode > 17.5) {
        c = crt_aperture(uv);
    } else if (u_mode > 16.5) {
        c = fakelottes(uv);
    } else if (u_mode > 15.5) {
        c = crt_geom_deluxe(uv);
    } else if (u_mode > 14.5) {
        c = crt_gdv_mini_ultra(uv);
    } else if (u_mode > 13.5) {
        // davej crt-pi defaults: scanlines, multisample, gamma, mask 1.
        vec2 tp = uv*u_src;
        float ty = floor(tp.y)+0.5;
        float dy = tp.y-ty;
        float fw = (1.0/crt_out_px().y)/3.0;
        float sw = (crtpi_weight(dy)+crtpi_weight(dy-fw)+crtpi_weight(dy+fw))*0.3333333;
        float sy = sign(dy);
        dy = dy*dy; dy = dy*dy; dy *= 8.0; dy /= u_src.y; dy *= sy;
        vec3 col = nearest_bilinear(vec2(uv.x, ty/u_src.y + dy));
        col = pow(col, vec3(2.4))*sw*1.5;
        col = pow(col, vec3(1.0/2.2));
        c = col*(fract(gl_FragCoord.x*0.5) < 0.5 ? vec3(0.7,1.0,0.7) : vec3(1.0,0.7,1.0));
    } else if (u_mode > 12.5) {
        // SoltanGris42 zfast_crt_standard, RetroArch's parameter defaults.
        vec2 p = uv*u_src;
        vec2 i = floor(p)+0.5;
        vec2 ff = p-i;
        p = (i+4.0*ff*ff*ff)/u_src;
        p.x = mix(p.x, uv.x, 0.30);
        float yy1 = ff.y*ff.y;
        float yy2 = yy1*yy1;
        float mask = 1.0 - (fract(floor(gl_FragCoord.x)*-0.5) < 0.5 ? 0.25 : 0.0);
        vec3 col = nearest_bilinear(p);
        float w1 = 1.25 - 6.0*(yy1-2.05*yy2);
        float w2 = 1.0 - 8.0*(yy2-2.8*yy2*yy1);
        c = col*mix(w1*mask, w2, dot(col, vec3(0.3333*0.8)));
    } else if (u_mode > 11.5) {
        // Themaister sharp-bilinear: automatic prescale from the shown rows.
        vec2 texel=uv*u_src;
        vec2 s=fract(texel);
        float scale=max(1.0,floor((u_rect.w*u_fbo.y)/(u_uv.w*u_src.y)+0.01));
        float bound=0.5-0.5/scale;
        vec2 delta=s-vec2(0.5);
        vec2 filtered=(delta-clamp(delta,vec2(-bound),vec2(bound)))*scale+0.5;
        c=nearest_bilinear((floor(texel)+filtered)/u_src);
    } else if (u_mode > 10.5) {
        // endrift AGS-001: frontlit SP gamma, subpixel mask and light.
        c=mgba_pass(uv,true);
        vec2 radius=pow(abs((v_uv-vec2(0.5))*1.5),vec2(4.0));
        vec3 bleed=vec3(0.12,0.14,0.19) +
            (dot(radius,radius)+vec3(0.02,0.03,0.05))*vec3(0.14,0.18,0.2);
        c+=bleed+0.07*mgba_pass(uv-vec2(0.0,0.025),true);
    } else if (u_mode > 9.5) {
        // endrift AGB-001: original non-backlit screen color subpixels.
        c=mgba_pass(uv,false);
    } else if (u_mode > 8.5) {
        // jdgleaver simpletex_lcd+gba-color, using Slot's existing
        // licensed paper texture crop. Russ (RetroGameCorps) settings,
        // Siham 2026-11-04: Background Intensity 0.80, Darken Colours
        // 0.25, Darken Grid 0.15, Grid Intensity 0.65.
        c=gba_simpletex_color(c);
        c=pow(c,vec3(1.25));
        vec2 dd=abs(f-vec2(0.5));
        float d2=max(dd.x,dd.y); d2*=d2;
        float d4=d2*d2;
        float weight=48.0*(d4-(8.0/3.0)*d4*d2);
        weight=weight*(weight+(1.0-weight)*0.65)*0.65;
        float lum=dot(c,luma_coeff);
        weight*=lum+(1.0-lum)*0.4;
        c=mix(c,vec3(0.85),weight);
        vec3 paper=texture2D(u_paper,
            (floor(gl_FragCoord.xy)+vec2(0.5))/u_paper_size).rgb;
        c=mix(c,paper*c,dot(c,luma_coeff)*0.80);
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
        // simpletex_lcd with Russ (RetroGameCorps) settings, Siham
        // 2026-11-04: Background Intensity 0.80, Darken Colours 0.25,
        // Darken Grid 0.15, Grid Intensity 0.65.
        c = pow(c, vec3(1.25));
        vec2 d = abs(f - 0.5);
        float x2 = max(d.x, d.y);
        x2 = x2 * x2;
        float x4 = x2 * x2;
        float w = 48.0 * (x4 - (8.0 / 3.0) * x4 * x2);
        w = w * (w + (1.0 - w) * 0.65) * 0.65;
        float luma = dot(c, luma_coeff);
        w = w * luma;
        c = mix(c, vec3(0.85), w);
        vec3 bg = texture2D(u_paper, (floor(gl_FragCoord.xy) + 0.5) / u_paper_size).rgb * c;
        c = mix(c, bg, dot(c, luma_coeff) * 0.80);
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
        c = grid_line(uv, c);
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

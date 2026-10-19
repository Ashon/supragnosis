// The GPU renderer (docs/design-system.md Section 27.9, U39). Where WebGL2 can render to half-float
// targets, the map's bulk is drawn here, on a canvas under the 2D one: the region field, the
// workspace outlines, the relations and their arrowheads, the nodes and the bodies. The viewer draws
// them with the calls it gives the 2D canvas - paths of moves, lines, curves, full circles and
// rectangles, filled or stroked in a style - and this renderer takes the same calls, keeps each fill
// and stroke as instances in the order they came, and draws them when the frame ends: discs, rings,
// squares and glows by their signed distance, lines and curves by their distance to the segment,
// and what else is filled as triangles. The field is added up in a half-float target and mixed in
// OKLCH in one pass, as the 2D field mixes it.
//
// What crosses the GPU differs from the 2D canvas in one way: the 2D canvas strokes a look's
// relations as one path, so two that cross are no darker where they cross; here each is its own
// shape, and the crossing adds up, as it does in every WebGL map.
"use strict";

// A 2D context for reading a CSS color into RGBA, once per color.
const GPU_COLOR = { ctx: null, cache: new Map() };
function gpuColor(css) {
  let c = GPU_COLOR.cache.get(css);
  if (c) return c;
  if (!GPU_COLOR.ctx) {
    const cv = document.createElement("canvas");
    cv.width = cv.height = 1;
    GPU_COLOR.ctx = cv.getContext("2d", { willReadFrequently: true });
  }
  const x = GPU_COLOR.ctx;
  x.clearRect(0, 0, 1, 1);
  x.fillStyle = "#000"; x.fillStyle = css;
  x.fillRect(0, 0, 1, 1);
  // Image data is read back straight, not premultiplied.
  const d = x.getImageData(0, 0, 1, 1).data;
  c = [d[0] / 255, d[1] / 255, d[2] / 255, d[3] / 255];
  if (GPU_COLOR.cache.size > 512) GPU_COLOR.cache.clear();
  GPU_COLOR.cache.set(css, c);
  return c;
}

// A path as the renderer keeps it: full circles, squares, polylines for strokes and polygons for
// fills, and quadratic curves.
class GpuPath {
  constructor() { this.circles = []; this.squares = []; this.subs = []; this.curves = []; this.cur = null; this.open = false; this.ax = 0; this.ay = 0; }
  // A move starts no polyline until a line is drawn from it, so a move to a circle costs nothing.
  moveTo(x, y) { this.cur = null; this.open = true; this.ax = x; this.ay = y; }
  lineTo(x, y) {
    if (!this.open) { this.moveTo(x, y); return; }
    if (!this.cur) { this.cur = { pts: [this.ax, this.ay], closed: false }; this.subs.push(this.cur); }
    this.cur.pts.push(x, y); this.ax = x; this.ay = y;
  }
  quadraticCurveTo(cx, cy, x, y) {
    if (!this.open) this.moveTo(cx, cy);
    this.curves.push(this.ax, this.ay, cx, cy, x, y);
    this.cur = null; this.ax = x; this.ay = y;
  }
  arc(x, y, r, a0, a1) {
    if (Math.abs(a1 - a0) >= 2 * Math.PI - 1e-6) {
      this.circles.push(x, y, r);
      this.cur = null; this.open = true; this.ax = x + r; this.ay = y;
      return;
    }
    const steps = Math.max(4, Math.ceil(Math.abs(a1 - a0) / 0.25));
    for (let k = 0; k <= steps; k++) {
      const t = a0 + (a1 - a0) * k / steps;
      this.lineTo(x + Math.cos(t) * r, y + Math.sin(t) * r);
    }
  }
  rect(x, y, w, h) {
    if (w === h) { this.squares.push(x + w / 2, y + h / 2, w / 2); this.cur = null; return; }
    this.moveTo(x, y); this.lineTo(x + w, y); this.lineTo(x + w, y + h); this.lineTo(x, y + h); this.closePath();
  }
  roundRect(x, y, w, h, r) {
    r = Math.max(0, Math.min(r, w / 2, h / 2));
    this.moveTo(x + r, y);
    const corner = (cx, cy, from) => { for (let k = 0; k <= 6; k++) { const t = from + k / 6 * Math.PI / 2; this.lineTo(cx + Math.cos(t) * r, cy + Math.sin(t) * r); } };
    corner(x + w - r, y + r, -Math.PI / 2); corner(x + w - r, y + h - r, 0);
    corner(x + r, y + h - r, Math.PI / 2); corner(x + r, y + r, Math.PI);
    this.closePath();
  }
  closePath() {
    if (this.cur) { this.cur.closed = true; this.ax = this.cur.pts[0]; this.ay = this.cur.pts[1]; }
    this.cur = null;
  }
}

const GPU_VS_DISC = `#version 300 es
layout(location=0) in vec2 a_corner;
layout(location=1) in vec4 a_geo;
layout(location=2) in vec4 a_col;
layout(location=3) in float a_kind;
uniform vec3 u_cam; uniform vec2 u_view; uniform float u_dpr;
out vec2 v_p; out vec2 v_r; out vec4 v_col; out float v_kind; out float v_aa;
void main() {
  float R = a_geo.w + 2.0 / u_cam.x;
  vec2 w = a_geo.xy + a_corner * R, s = w * u_cam.x + u_cam.yz;
  gl_Position = vec4(s.x / u_view.x * 2.0 - 1.0, 1.0 - s.y / u_view.y * 2.0, 0.0, 1.0);
  v_p = a_corner * R; v_r = a_geo.zw; v_col = a_col; v_kind = a_kind; v_aa = 1.0 / (u_cam.x * u_dpr);
}`;
// 0 a disc of radius r1, 1 a square of half side r1, 2 a ring from r0 to r1, 3 a glow, whole to r0
// and clear by r1.
const GPU_FS_DISC = `#version 300 es
precision highp float;
in vec2 v_p; in vec2 v_r; in vec4 v_col; in float v_kind; in float v_aa;
out vec4 o;
void main() {
  float l = length(v_p), a;
  if (v_kind < 0.5) a = clamp(0.5 - (l - v_r.y) / v_aa, 0.0, 1.0);
  else if (v_kind < 1.5) { vec2 q = abs(v_p) - v_r.y; a = clamp(0.5 - max(q.x, q.y) / v_aa, 0.0, 1.0); }
  else if (v_kind < 2.5) a = clamp(0.5 - max(v_r.x - l, l - v_r.y) / v_aa, 0.0, 1.0);
  else a = 1.0 - clamp((l - v_r.x) / max(v_r.y - v_r.x, 1e-6), 0.0, 1.0);
  if (a <= 0.0) discard;
  o = v_col * a;
}`;
// A relation: a capsule from p0 to p1, or a quadratic curve through its control point, as wide as
// twice its half width; the shape is laid along p0 to p1 and reaches past the control point.
const GPU_VS_LINE = `#version 300 es
layout(location=0) in vec2 a_corner;
layout(location=1) in vec4 a_seg;
layout(location=2) in vec4 a_col;
layout(location=3) in vec3 a_ctl;
uniform vec3 u_cam; uniform vec2 u_view; uniform float u_dpr;
out vec2 v_q; out vec2 v_b; out vec2 v_c; out float v_hw; out vec4 v_col; out float v_curve; out float v_aa;
void main() {
  vec2 p0 = a_seg.xy, p1 = a_seg.zw, d = p1 - p0;
  float L = length(d);
  vec2 u = L > 1e-6 ? d / L : vec2(1.0, 0.0), n = vec2(-u.y, u.x);
  float pad = a_ctl.z + 2.0 / u_cam.x;
  vec2 c = a_ctl.xy - p0;
  float ct = dot(c, u), cn = dot(c, n);
  float t0 = min(0.0, ct) - pad, t1 = max(L, ct) + pad, n0 = min(0.0, cn) - pad, n1 = max(0.0, cn) + pad;
  vec2 k = a_corner * 0.5 + 0.5;
  vec2 rel = u * mix(t0, t1, k.x) + n * mix(n0, n1, k.y), w = p0 + rel, s = w * u_cam.x + u_cam.yz;
  gl_Position = vec4(s.x / u_view.x * 2.0 - 1.0, 1.0 - s.y / u_view.y * 2.0, 0.0, 1.0);
  v_q = rel; v_b = c; v_c = d; v_hw = a_ctl.z; v_col = a_col;
  v_curve = abs(cn) > 1e-3 ? 1.0 : 0.0; v_aa = 1.0 / (u_cam.x * u_dpr);
}`;
const GPU_FS_LINE = `#version 300 es
precision highp float;
in vec2 v_q; in vec2 v_b; in vec2 v_c; in float v_hw; in vec4 v_col; in float v_curve; in float v_aa;
out vec4 o;
float dot2(vec2 v) { return dot(v, v); }
// The distance to a quadratic Bezier from A (here the origin) through B to C (Inigo Quilez).
float bezier(vec2 pos, vec2 B, vec2 C) {
  vec2 a = B, b = -2.0 * B + C, c = a * 2.0, d = -pos;
  float kk = 1.0 / dot(b, b), kx = kk * dot(a, b), ky = kk * (2.0 * dot(a, a) + dot(d, b)) / 3.0, kz = kk * dot(d, a);
  float p = ky - kx * kx, q = kx * (2.0 * kx * kx - 3.0 * ky) + kz, h = q * q + 4.0 * p * p * p, res;
  if (h >= 0.0) {
    h = sqrt(h);
    vec2 x = (vec2(h, -h) - q) / 2.0, uv = sign(x) * pow(abs(x), vec2(1.0 / 3.0));
    float t = clamp(uv.x + uv.y - kx, 0.0, 1.0);
    res = dot2(d + (c + b * t) * t);
  } else {
    float z = sqrt(-p), v = acos(q / (p * z * 2.0)) / 3.0, m = cos(v), n = sin(v) * 1.732050808;
    vec3 t = clamp(vec3(m + m, -n - m, n - m) * z - kx, 0.0, 1.0);
    res = min(dot2(d + (c + b * t.x) * t.x), dot2(d + (c + b * t.y) * t.y));
  }
  return sqrt(res);
}
void main() {
  float dist;
  if (v_curve > 0.5) dist = bezier(v_q, v_b, v_c);
  else {
    float L2 = dot(v_c, v_c), t = L2 > 0.0 ? clamp(dot(v_q, v_c) / L2, 0.0, 1.0) : 0.0;
    dist = length(v_q - v_c * t);
  }
  float a = clamp(0.5 - (dist - v_hw) / v_aa, 0.0, 1.0);
  if (a <= 0.0) discard;
  o = v_col * a;
}`;
const GPU_VS_TRI = `#version 300 es
layout(location=0) in vec2 a_pos;
layout(location=1) in vec4 a_col;
uniform vec3 u_cam; uniform vec2 u_view;
out vec4 v_col;
void main() {
  vec2 s = a_pos * u_cam.x + u_cam.yz;
  gl_Position = vec4(s.x / u_view.x * 2.0 - 1.0, 1.0 - s.y / u_view.y * 2.0, 0.0, 1.0);
  v_col = a_col;
}`;
const GPU_FS_TRI = `#version 300 es
precision highp float;
in vec4 v_col; out vec4 o;
void main() { o = v_col; }`;
// The region field: each cloud an ellipse adding its weight, its hue's vector and the log of what it
// leaves uncovered, by the falloff the 2D field uses (cloudFalloff in viewer.js).
const GPU_VS_CLOUD = `#version 300 es
layout(location=0) in vec2 a_corner;
layout(location=1) in vec4 a_c;
layout(location=2) in vec4 a_t;
uniform vec3 u_cam; uniform vec2 u_view;
out vec2 v_uv; out vec3 v_w;
void main() {
  float cs = cos(a_t.x), sn = sin(a_t.x);
  vec2 l = a_corner * a_c.zw, w = a_c.xy + vec2(l.x * cs - l.y * sn, l.x * sn + l.y * cs), s = w * u_cam.x + u_cam.yz;
  gl_Position = vec4(s.x / u_view.x * 2.0 - 1.0, 1.0 - s.y / u_view.y * 2.0, 0.0, 1.0);
  v_uv = a_corner; v_w = a_t.yzw;
}`;
const GPU_FS_CLOUD = `#version 300 es
precision highp float;
in vec2 v_uv; in vec3 v_w; out vec4 o;
float falloff(float r) {
  return r < 0.45 ? 1.0 - 0.2 * r / 0.45 : r < 0.75 ? 0.8 - 0.42 * (r - 0.45) / 0.3 : 0.38 * (1.0 - r) / 0.25;
}
void main() {
  float r2 = dot(v_uv, v_uv);
  if (r2 >= 1.0) discard;
  float wt = v_w.x * falloff(sqrt(r2));
  o = vec4(wt, wt * v_w.y, wt * v_w.z, log(max(1.0 - wt, 1e-4)));
}`;
const GPU_VS_FULL = `#version 300 es
out vec2 v_uv;
void main() {
  vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0);
  v_uv = p * 0.5 + 0.5;
  gl_Position = vec4(p, 0.0, 1.0);
}`;
// The mix, as drawCloudField's: one lightness and chroma, the hue the clouds' weighted circular mean
// with the chroma eased only where the hues oppose, and the coverage built as layered paint would,
// eased toward its ceiling.
const GPU_FS_RESOLVE = `#version 300 es
precision highp float;
in vec2 v_uv; out vec4 o;
uniform sampler2D u_field; uniform float u_L; uniform float u_C; uniform float u_max;
float gamma(float v) { v = clamp(v, 0.0, 1.0); return v <= 0.0031308 ? 12.92 * v : 1.055 * pow(v, 1.0 / 2.4) - 0.055; }
void main() {
  vec4 t = texture(u_field, v_uv);
  float W = t.x;
  if (W <= 1e-5) discard;
  float vec = length(t.yz) / W, c = vec > 1e-6 ? u_C * pow(min(1.0, vec), 0.35) / (vec * W) : 0.0;
  float A = t.y * c, B = t.z * c;
  float l = u_L + 0.3963377774 * A + 0.2158037573 * B, m = u_L - 0.1055613458 * A - 0.0638541728 * B, s = u_L - 0.0894841775 * A - 1.2914855480 * B;
  l = l * l * l; m = m * m * m; s = s * s * s;
  vec3 rgb = vec3(gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s));
  float cover = u_max * (1.0 - exp(-(1.0 - exp(t.w)) / u_max));
  o = vec4(rgb * cover, cover);
}`;

// The renderer for a canvas, or null where WebGL2 cannot render to half-float targets. `onLost` is
// called once the context is lost; the viewer then draws with the 2D canvas alone.
function createGpu(canvas, onLost) {
  let gl = null;
  try { gl = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: true, antialias: true }); } catch (e) { gl = null; }
  if (!gl || !gl.getExtension("EXT_color_buffer_float")) return null;
  let lost = false;
  canvas.addEventListener("webglcontextlost", ev => { ev.preventDefault(); lost = true; onLost(); });

  const compile = (vs, fs) => {
    const p = gl.createProgram();
    for (const [type, src] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) {
      const sh = gl.createShader(type);
      gl.shaderSource(sh, src); gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh) || "shader");
      gl.attachShader(p, sh);
    }
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) || "program");
    const u = {};
    for (const name of ["u_cam", "u_view", "u_dpr", "u_field", "u_L", "u_C", "u_max"]) u[name] = gl.getUniformLocation(p, name);
    return { p, u };
  };
  let progs;
  try {
    progs = {
      disc: compile(GPU_VS_DISC, GPU_FS_DISC), line: compile(GPU_VS_LINE, GPU_FS_LINE), tri: compile(GPU_VS_TRI, GPU_FS_TRI),
      cloud: compile(GPU_VS_CLOUD, GPU_FS_CLOUD), resolve: compile(GPU_VS_FULL, GPU_FS_RESOLVE),
    };
  } catch (e) { return null; }

  // A unit quad's corners, shared by the instanced programs.
  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  // Each program's instances, grown as a frame needs; `stride` floats each.
  const store = (stride, layout) => {
    const vao = gl.createVertexArray(), buf = gl.createBuffer();
    return { stride, layout, vao, buf, data: new Float32Array(stride * 1024), n: 0 };
  };
  const discs = store(9, [[1, 4], [2, 4], [3, 1]]);
  const lines = store(11, [[1, 4], [2, 4], [3, 3]]);
  const tris = store(6, null);
  const clouds = store(8, [[1, 4], [2, 4]]);
  // Room for one more instance; the offset it is written at.
  const slot = s => {
    if ((s.n + 1) * s.stride > s.data.length) { const d = new Float32Array(s.data.length * 2); d.set(s.data); s.data = d; }
    return s.n++ * s.stride;
  };
  const disc = (x, y, r0, r1, c, kind) => {
    const o = slot(discs), d = discs.data;
    d[o] = x; d[o + 1] = y; d[o + 2] = r0; d[o + 3] = r1; d[o + 4] = c[0]; d[o + 5] = c[1]; d[o + 6] = c[2]; d[o + 7] = c[3]; d[o + 8] = kind;
  };
  const line = (x0, y0, x1, y1, c, cx, cy, hw) => {
    const o = slot(lines), d = lines.data;
    d[o] = x0; d[o + 1] = y0; d[o + 2] = x1; d[o + 3] = y1; d[o + 4] = c[0]; d[o + 5] = c[1]; d[o + 6] = c[2]; d[o + 7] = c[3];
    d[o + 8] = cx; d[o + 9] = cy; d[o + 10] = hw;
  };
  const vertex = (x, y, c) => {
    const o = slot(tris), d = tris.data;
    d[o] = x; d[o + 1] = y; d[o + 2] = c[0]; d[o + 3] = c[1]; d[o + 4] = c[2]; d[o + 5] = c[3];
  };
  // The order the instances came in: runs of one program.
  let runs = [];
  const run = (s, from) => {
    if (s.n === from) return;
    const last = runs[runs.length - 1];
    if (last && last.s === s && last.end === from) last.end = s.n; else runs.push({ s, start: from, end: s.n });
  };

  // The field's target, a cell for every 2 CSS pixels each way: a cloud has no detail to lose.
  const FIELD_CELL = 2;
  let fieldTex = null, fieldFbo = null, fieldW = 0, fieldH = 0, fieldLook = null;

  let view = { s: 1, x: 0, y: 0, w: 1, h: 1, dpr: 1 };
  // The 2D context's state the viewer sets, and its paths.
  const st = { globalAlpha: 1, fillStyle: "#000", strokeStyle: "#000", lineWidth: 1 };
  const stack = [];
  let path = new GpuPath();
  const colorOf = (style, alpha) => {
    const c = gpuColor(style), a = c[3] * alpha;
    return [c[0] * a, c[1] * a, c[2] * a, a];
  };
  const fill = p => {
    const g = typeof st.fillStyle === "object" && st.fillStyle !== null ? st.fillStyle : null;
    if (g) {
      // A radial gradient fills a glow: whole at its inner radius, clear at its outer.
      const c = colorOf(g.color, st.globalAlpha), from = discs.n;
      for (let i = 0; i < p.circles.length; i += 3) disc(p.circles[i], p.circles[i + 1], g.r0, g.r1, c, 3);
      run(discs, from);
      return;
    }
    const c = colorOf(st.fillStyle, st.globalAlpha);
    if (c[3] <= 0) return;
    if (p.circles.length || p.squares.length) {
      const from = discs.n;
      for (let i = 0; i < p.circles.length; i += 3) disc(p.circles[i], p.circles[i + 1], 0, p.circles[i + 2], c, 0);
      for (let i = 0; i < p.squares.length; i += 3) disc(p.squares[i], p.squares[i + 1], 0, p.squares[i + 2], c, 1);
      run(discs, from);
    }
    const from = tris.n;
    for (const sub of p.subs) {
      const q = sub.pts;
      for (let i = 2; i + 3 < q.length; i += 2) {
        vertex(q[0], q[1], c); vertex(q[i], q[i + 1], c); vertex(q[i + 2], q[i + 3], c);
      }
    }
    if (tris.n > from) run(tris, from);
  };
  const stroke = p => {
    const c = colorOf(st.strokeStyle, st.globalAlpha), hw = st.lineWidth / 2;
    if (c[3] <= 0) return;
    if (p.circles.length) {
      const from = discs.n;
      for (let i = 0; i < p.circles.length; i += 3) disc(p.circles[i], p.circles[i + 1], p.circles[i + 2] - hw, p.circles[i + 2] + hw, c, 2);
      run(discs, from);
    }
    const from = lines.n;
    const seg = (x0, y0, x1, y1) => line(x0, y0, x1, y1, c, (x0 + x1) / 2, (y0 + y1) / 2, hw);
    for (const sub of p.subs) {
      const q = sub.pts;
      for (let i = 0; i + 3 < q.length; i += 2) seg(q[i], q[i + 1], q[i + 2], q[i + 3]);
      if (sub.closed && q.length >= 4) seg(q[q.length - 2], q[q.length - 1], q[0], q[1]);
    }
    for (let i = 0; i < p.squares.length; i += 3) {
      const x = p.squares[i], y = p.squares[i + 1], r = p.squares[i + 2];
      seg(x - r, y - r, x + r, y - r); seg(x + r, y - r, x + r, y + r); seg(x + r, y + r, x - r, y + r); seg(x - r, y + r, x - r, y - r);
    }
    const k = p.curves;
    for (let i = 0; i < k.length; i += 6) line(k[i], k[i + 1], k[i + 4], k[i + 5], c, k[i + 2], k[i + 3], hw);
    if (lines.n > from) run(lines, from);
  };

  const draw = (s, prog) => {
    gl.useProgram(prog.p);
    gl.uniform3f(prog.u.u_cam, view.s, view.x, view.y);
    gl.uniform2f(prog.u.u_view, view.w, view.h);
    if (prog.u.u_dpr) gl.uniform1f(prog.u.u_dpr, view.dpr);
    gl.bindVertexArray(s.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, s.buf);
    gl.bufferData(gl.ARRAY_BUFFER, s.data.subarray(0, s.n * s.stride), gl.STREAM_DRAW);
  };
  // Points a program's per-instance attributes at its run's first instance.
  const point = (s, start) => {
    const bytes = s.stride * 4;
    let off = start * bytes;
    for (const [loc, size] of s.layout) {
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, bytes, off);
      gl.vertexAttribDivisor(loc, 1);
      off += size * 4;
    }
  };
  const corners = s => {
    gl.bindVertexArray(s.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 8, 0);
    gl.vertexAttribDivisor(0, 0);
  };
  for (const s of [discs, lines, clouds]) corners(s);

  const ensureField = (w, h) => {
    if (fieldTex && fieldW === w && fieldH === h) return;
    if (fieldTex) { gl.deleteTexture(fieldTex); gl.deleteFramebuffer(fieldFbo); }
    fieldTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, fieldTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    fieldFbo = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fieldFbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, fieldTex, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    fieldW = w; fieldH = h;
  };
  const drawField = () => {
    if (!clouds.n || !fieldLook) return;
    const w = Math.max(1, Math.ceil(view.w / FIELD_CELL)), h = Math.max(1, Math.ceil(view.h / FIELD_CELL));
    ensureField(w, h);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fieldFbo);
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
    gl.blendFunc(gl.ONE, gl.ONE);
    draw(clouds, progs.cloud);
    point(clouds, 0);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, clouds.n);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.useProgram(progs.resolve.p);
    gl.bindVertexArray(null);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, fieldTex);
    gl.uniform1i(progs.resolve.u.u_field, 0);
    gl.uniform1f(progs.resolve.u.u_L, fieldLook.L); gl.uniform1f(progs.resolve.u.u_C, fieldLook.C); gl.uniform1f(progs.resolve.u.u_max, fieldLook.coverMax);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  return {
    get lost() { return lost; },
    // The frame begins: the camera, the window in CSS pixels and the device's pixel ratio.
    begin(v) {
      view = v;
      discs.n = lines.n = tris.n = clouds.n = 0; runs = []; fieldLook = null;
      st.globalAlpha = 1; stack.length = 0; path = new GpuPath();
    },
    // The region field of the frame: each item's cloud shape, strength and hue, faded as the 2D field
    // fades a cloud wider than `fadeAt` of the window.
    field(items, look) {
      fieldLook = look;
      const fit = look.fadeAt * Math.min(view.w, view.h);
      for (const it of items) {
        if (it.alpha <= 0) continue;
        const sh = it.shape, across = Math.max(sh.R, sh.Rv) * view.s;
        const fade = across > fit ? Math.max(0.15, (fit / across) ** 1.5) : 1, hue = it.hue * Math.PI / 180;
        const px = sh.cx * view.s + view.x, py = sh.cy * view.s + view.y, reach = Math.max(sh.R, sh.Rv) * view.s;
        if (px + reach < 0 || px - reach > view.w || py + reach < 0 || py - reach > view.h) continue;
        const o = slot(clouds), d = clouds.data;
        d[o] = sh.cx; d[o + 1] = sh.cy; d[o + 2] = sh.R; d[o + 3] = sh.Rv; d[o + 4] = sh.th; d[o + 5] = it.alpha * fade; d[o + 6] = Math.cos(hue); d[o + 7] = Math.sin(hue);
      }
    },
    // The frame ends: everything drawn, in the order it came.
    end() {
      if (lost) return;
      const W = Math.round(view.w * view.dpr), H = Math.round(view.h * view.dpr);
      if (canvas.width !== W || canvas.height !== H) { canvas.width = W; canvas.height = H; }
      gl.viewport(0, 0, W, H);
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
      gl.enable(gl.BLEND);
      drawField();
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      const uploaded = new Set();
      for (const r of runs) {
        const s = r.s, prog = s === discs ? progs.disc : s === lines ? progs.line : progs.tri;
        if (!uploaded.has(s)) { draw(s, prog); uploaded.add(s); }
        else {
          gl.useProgram(prog.p); gl.bindVertexArray(s.vao); gl.bindBuffer(gl.ARRAY_BUFFER, s.buf);
        }
        if (s === tris) {
          const bytes = 24;
          gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, bytes, 0); gl.vertexAttribDivisor(0, 0);
          gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 4, gl.FLOAT, false, bytes, 8); gl.vertexAttribDivisor(1, 0);
          gl.drawArrays(gl.TRIANGLES, r.start, r.end - r.start);
        } else {
          point(s, r.start);
          gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, r.end - r.start);
        }
      }
      gl.bindVertexArray(null);
    },
    // Nothing on the GPU's canvas: behind the loader.
    clear() {
      if (lost) return;
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
    },
    path: () => new GpuPath(),
    // What the last frame held, for a measure: instances of each kind and the runs they were drawn in.
    stats: () => ({ discs: discs.n, lines: lines.n, triangles: tris.n / 3, clouds: clouds.n, runs: runs.length }),
    // The 2D context's calls the viewer gives the map's bulk.
    get globalAlpha() { return st.globalAlpha; }, set globalAlpha(v) { st.globalAlpha = v; },
    get fillStyle() { return st.fillStyle; }, set fillStyle(v) { st.fillStyle = v; },
    get strokeStyle() { return st.strokeStyle; }, set strokeStyle(v) { st.strokeStyle = v; },
    get lineWidth() { return st.lineWidth; }, set lineWidth(v) { st.lineWidth = v; },
    lineCap: "round", lineJoin: "round",
    save() { stack.push({ ...st }); },
    restore() { const s = stack.pop(); if (s) Object.assign(st, s); },
    beginPath() { path = new GpuPath(); },
    moveTo(x, y) { path.moveTo(x, y); },
    lineTo(x, y) { path.lineTo(x, y); },
    quadraticCurveTo(cx, cy, x, y) { path.quadraticCurveTo(cx, cy, x, y); },
    arc(x, y, r, a0, a1) { path.arc(x, y, r, a0, a1); },
    rect(x, y, w, h) { path.rect(x, y, w, h); },
    roundRect(x, y, w, h, r) { path.roundRect(x, y, w, h, r); },
    closePath() { path.closePath(); },
    fill(p) { fill(p || path); },
    stroke(p) { stroke(p || path); },
    fillRect(x, y, w, h) { const p = new GpuPath(); p.rect(x, y, w, h); fill(p); },
    // A radial gradient as the viewer uses one: a color at its inner radius, clear at its outer.
    createRadialGradient(x0, y0, r0, x1, y1, r1) {
      return { r0, r1, color: "#000", addColorStop(at, color) { if (at === 0) this.color = color; } };
    },
  };
}

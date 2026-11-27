// WebGL port of the Android 12+ patterned ripple (AOSP `RippleShader`): a soft
// radial wave that drifts from the touch point to the host center, with a ring
// of turbulence-masked sparkles that fades out before the press settles.

const VERTEX_SHADER = `
attribute vec2 a_position;

void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`

const FRAGMENT_SHADER = `
precision highp float;

uniform float u_canvasHeight;
uniform vec2 u_size;
uniform vec2 u_origin;
uniform vec2 u_touch;
uniform float u_progress;
uniform float u_maxRadius;
uniform vec2 u_noiseScale;
uniform float u_noisePhase;
uniform vec2 u_tCircle1;
uniform vec2 u_tCircle2;
uniform vec2 u_tCircle3;
uniform vec2 u_tRotation1;
uniform vec2 u_tRotation2;
uniform vec2 u_tRotation3;
uniform vec3 u_color;
uniform vec4 u_sparkleColor;
uniform float u_alpha;
uniform float u_easeFadeOut;

const float PI = 3.1415926535897932384626;

float triangleNoise(vec2 n) {
  n = fract(n * vec2(5.3987, 5.4421));
  n += dot(n.yx, n.xy + vec2(21.5351, 14.3137));
  float xy = n.x * n.y;
  return fract(xy * 95.4307) + fract(xy * 75.04961) - 1.0;
}

float threshold(float v, float l, float h) {
  return step(l, v) * (1.0 - step(h, v));
}

float sparkles(vec2 uv, float t) {
  float n = triangleNoise(uv);
  float s = 0.0;
  for (float i = 0.0; i < 4.0; i += 1.0) {
    float l = i * 0.1;
    float h = l + 0.05;
    float o = sin(PI * (t + 0.35 * i));
    s += threshold(n + o, l, h);
  }
  return clamp(s, 0.0, 1.0) * u_sparkleColor.a;
}

float softCircle(vec2 uv, vec2 xy, float radius, float blur) {
  float blurHalf = blur * 0.5;
  float d = distance(uv, xy);
  return 1.0 - smoothstep(1.0 - blurHalf, 1.0 + blurHalf, d / radius);
}

float softRing(vec2 uv, vec2 xy, float radius, float progress, float blur) {
  float thickness = 0.05 * radius;
  float currentRadius = radius * progress;
  float outer = softCircle(uv, xy, currentRadius + thickness, blur);
  float inner = softCircle(uv, xy, max(currentRadius - thickness, 0.0), blur);
  return clamp(outer - inner, 0.0, 1.0);
}

float subProgress(float start, float end, float progress) {
  return (clamp(progress, start, end) - start) / (end - start);
}

mat2 rotate2d(vec2 rad) {
  return mat2(rad.x, -rad.y, rad.y, rad.x);
}

float circleGrid(vec2 resolution, vec2 coord, vec2 center, vec2 rotation,
    float cellDiameter) {
  coord = rotate2d(rotation) * (center - coord) + center;
  coord = mod(coord, cellDiameter) / resolution;
  float normalRadius = cellDiameter / resolution.y * 0.5;
  float radius = 0.65 * normalRadius;
  return softCircle(coord, vec2(normalRadius), radius, radius * 50.0);
}

float turbulence(vec2 uv) {
  const vec2 scale = vec2(0.8);
  uv = uv * scale;
  float g1 = circleGrid(scale, uv, u_tCircle1, u_tRotation1, 0.17);
  float g2 = circleGrid(scale, uv, u_tCircle2, u_tRotation2, 0.2);
  float g3 = circleGrid(scale, uv, u_tCircle3, u_tRotation3, 0.275);
  float v = (g1 * g1 + g2 - g3) * 0.5;
  return clamp(0.45 + 0.8 * v, 0.0, 1.0);
}

void main() {
  // Device pixels from the top-left corner of the host.
  vec2 p = vec2(gl_FragCoord.x, u_canvasHeight - gl_FragCoord.y);

  float fadeIn = subProgress(0.0, 0.13, u_progress);
  float scaleIn = subProgress(0.0, 1.0, u_progress);
  float fadeOutNoise = subProgress(0.4, 0.5, u_progress);
  float fadeOutRipple = subProgress(0.4, 1.0, u_progress);
  vec2 center = mix(u_touch, u_origin, clamp(u_progress * 2.0, 0.0, 1.0));

  float ring = softRing(p, center, u_maxRadius, scaleIn, 1.0);
  float alpha = min(fadeIn, 1.0 - fadeOutNoise);
  vec2 uv = p / u_size;
  vec2 densityUv = uv - mod(uv, u_noiseScale);
  float sparkleAlpha =
      sparkles(densityUv, u_noisePhase) * ring * alpha * turbulence(uv);

  float fadeOut = 1.0 - fadeOutRipple;
  fadeOut = mix(fadeOut, fadeOut * (2.0 - fadeOut), u_easeFadeOut);
  float fade = min(fadeIn, fadeOut);
  float waveAlpha =
      softCircle(p, center, u_maxRadius * scaleIn, 1.0) * fade * u_alpha;
  vec4 waveColor = vec4(u_color * waveAlpha, waveAlpha);
  vec4 sparkleColor =
      vec4(u_sparkleColor.rgb * u_sparkleColor.a, u_sparkleColor.a);

  // The color's alpha only tints the wave; sparkles keep their own alpha.
  gl_FragColor = mix(waveColor, sparkleColor, sparkleAlpha);
}
`

/** Sparkle cell edge in device pixels (AOSP `RippleShader` density scale). */
const NOISE_CELL_SIZE = 2.1
/** AOSP scales the bounded hotspot radius before handing it to the shader. */
const MAX_RADIUS_SCALE = 2.3
/** AOSP advances the noise phase by 7000 / 214 units over 7 seconds. */
const NOISE_PHASE_RATE = Math.floor(7000 / 214) / 7000
const TURBULENCE_SCALE = 1.5
const ROTATE_STEP = Math.PI * 0.0078125

export type RippleColor = readonly [r: number, g: number, b: number, a: number]

export interface RippleFrame {
  /** Touch point in CSS pixels relative to the host. */
  x: number
  y: number
  /** 0..0.5 while entering and held, 0.5..1 while exiting. */
  progress: number
  /** `performance.now()` when the ripple started; seeds the sparkle noise. */
  startedAt: number
  /** Milliseconds since the ripple started; drifts the sparkle noise. */
  elapsed: number
  /** Straight-alpha wave color in the 0..1 range. */
  color: RippleColor
  sparkleColor: RippleColor
  /**
   * Eases the wave's linear fade-out so its tail keeps twice the alpha while
   * still ending smoothly. AOSP fades linearly.
   */
  easeFadeOut: boolean
}

export interface RippleTarget {
  context: CanvasRenderingContext2D
  /** Host size in CSS pixels. */
  width: number
  height: number
  pixelRatio: number
}

export interface RippleRenderer {
  /** Returns `false` once the WebGL context is lost and the caller should fall back. */
  render: (target: RippleTarget, frames: readonly RippleFrame[]) => boolean
}

type UniformName =
  | 'u_canvasHeight'
  | 'u_size'
  | 'u_origin'
  | 'u_touch'
  | 'u_progress'
  | 'u_maxRadius'
  | 'u_noiseScale'
  | 'u_noisePhase'
  | 'u_tCircle1'
  | 'u_tCircle2'
  | 'u_tCircle3'
  | 'u_tRotation1'
  | 'u_tRotation2'
  | 'u_tRotation3'
  | 'u_color'
  | 'u_sparkleColor'
  | 'u_alpha'
  | 'u_easeFadeOut'

const UNIFORM_NAMES: readonly UniformName[] = [
  'u_canvasHeight',
  'u_size',
  'u_origin',
  'u_touch',
  'u_progress',
  'u_maxRadius',
  'u_noiseScale',
  'u_noisePhase',
  'u_tCircle1',
  'u_tCircle2',
  'u_tCircle3',
  'u_tRotation1',
  'u_tRotation2',
  'u_tRotation3',
  'u_color',
  'u_sparkleColor',
  'u_alpha',
  'u_easeFadeOut',
]

function compileShader(
  gl: WebGLRenderingContext,
  type: number,
  source: string,
): WebGLShader | null {
  const shader = gl.createShader(type)
  if (!shader) {
    return null
  }

  gl.shaderSource(shader, source)
  gl.compileShader(shader)

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader)
    return null
  }

  return shader
}

function createProgram(gl: WebGLRenderingContext): WebGLProgram | null {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER)
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER)
  const program = vertex && fragment ? gl.createProgram() : null

  if (program && vertex && fragment) {
    gl.attachShader(program, vertex)
    gl.attachShader(program, fragment)
    gl.bindAttribLocation(program, 0, 'a_position')
    gl.linkProgram(program)
  }

  if (vertex) gl.deleteShader(vertex)
  if (fragment) gl.deleteShader(fragment)

  if (program && !gl.getProgramParameter(program, gl.LINK_STATUS)) {
    gl.deleteProgram(program)
    return null
  }

  return program
}

function createRippleRenderer(): RippleRenderer | null {
  const canvas = document.createElement('canvas')
  const gl = canvas.getContext('webgl', {
    alpha: true,
    antialias: false,
    depth: false,
    premultipliedAlpha: true,
    preserveDrawingBuffer: false,
    stencil: false,
  })
  const program = gl ? createProgram(gl) : null

  if (!gl || !program) {
    return null
  }

  let lost = false
  canvas.addEventListener('webglcontextlost', () => {
    lost = true
    // Let the next ripple try a fresh context instead of reusing a dead one.
    sharedRenderer = undefined
  })

  const uniforms = Object.fromEntries(
    UNIFORM_NAMES.map((name) => [name, gl.getUniformLocation(program, name)]),
  ) as Record<UniformName, WebGLUniformLocation | null>

  gl.useProgram(program)
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer())
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
    gl.STATIC_DRAW,
  )
  gl.enableVertexAttribArray(0)
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
  gl.enable(gl.BLEND)
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
  gl.clearColor(0, 0, 0, 0)

  const setPhase = (phase: number) => {
    const s = TURBULENCE_SCALE
    const circle = (
      location: WebGLUniformLocation | null,
      offset: number,
      speed: number,
      angle: number,
    ) =>
      gl.uniform2f(
        location,
        s * offset + phase * speed * Math.cos(s * angle),
        s * offset + phase * speed * Math.sin(s * angle),
      )
    const rotation = (location: WebGLUniformLocation | null, angle: number) =>
      gl.uniform2f(location, Math.cos(angle), Math.sin(angle))

    gl.uniform1f(uniforms.u_noisePhase, phase * 0.001)
    circle(uniforms.u_tCircle1, 0.5, 0.01, 0.55)
    circle(uniforms.u_tCircle2, 0.2, -0.0066, 0.45)
    circle(uniforms.u_tCircle3, 1, -0.0066, 0.35)
    rotation(uniforms.u_tRotation1, phase * ROTATE_STEP + 1.7 * Math.PI)
    rotation(uniforms.u_tRotation2, phase * -ROTATE_STEP + 2 * Math.PI)
    rotation(uniforms.u_tRotation3, phase * ROTATE_STEP + 2.75 * Math.PI)
  }

  return {
    render: ({ context, width, height, pixelRatio }, frames) => {
      if (lost || gl.isContextLost()) {
        return false
      }

      const w = Math.max(1, Math.round(width * pixelRatio))
      const h = Math.max(1, Math.round(height * pixelRatio))

      // Share one drawing buffer between hosts: grow it on demand and draw
      // each host into its top-left corner.
      if (canvas.width < w || canvas.height < h) {
        canvas.width = Math.max(canvas.width, w)
        canvas.height = Math.max(canvas.height, h)
      }

      gl.viewport(0, canvas.height - h, w, h)
      gl.enable(gl.SCISSOR_TEST)
      gl.scissor(0, canvas.height - h, w, h)
      gl.clear(gl.COLOR_BUFFER_BIT)

      gl.uniform1f(uniforms.u_canvasHeight, canvas.height)
      gl.uniform2f(uniforms.u_size, w, h)
      gl.uniform2f(uniforms.u_origin, w / 2, h / 2)
      gl.uniform1f(
        uniforms.u_maxRadius,
        (Math.hypot(w, h) / 2) * MAX_RADIUS_SCALE,
      )
      gl.uniform2f(
        uniforms.u_noiseScale,
        NOISE_CELL_SIZE / w,
        NOISE_CELL_SIZE / h,
      )

      for (const frame of frames) {
        gl.uniform2f(
          uniforms.u_touch,
          frame.x * pixelRatio,
          frame.y * pixelRatio,
        )
        gl.uniform1f(uniforms.u_progress, frame.progress)
        gl.uniform3f(
          uniforms.u_color,
          frame.color[0],
          frame.color[1],
          frame.color[2],
        )
        gl.uniform1f(uniforms.u_alpha, frame.color[3])
        gl.uniform1f(uniforms.u_easeFadeOut, frame.easeFadeOut ? 1 : 0)
        gl.uniform4fv(uniforms.u_sparkleColor, frame.sparkleColor)
        // AOSP seeds the phase with the start time in ms, so every ripple
        // gets its own sparkle pattern that barely drifts while it plays.
        setPhase(frame.startedAt + frame.elapsed * NOISE_PHASE_RATE)
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
      }

      gl.disable(gl.SCISSOR_TEST)

      if (context.canvas.width !== w || context.canvas.height !== h) {
        context.canvas.width = w
        context.canvas.height = h
      }

      context.clearRect(0, 0, w, h)
      context.drawImage(canvas, 0, 0, w, h, 0, 0, w, h)

      return true
    },
  }
}

let sharedRenderer: RippleRenderer | null | undefined

/**
 * One WebGL context serves every ripple host, so hosts never compete for the
 * browser's context limit. `null` means WebGL is unavailable.
 */
export function getRippleRenderer(): RippleRenderer | null {
  if (sharedRenderer === undefined) {
    sharedRenderer = createRippleRenderer()
  }

  return sharedRenderer
}

/** Resolves any CSS color (including `oklch()`) to straight-alpha RGBA. */
export function resolveRippleColor(cssColor: string): RippleColor {
  const probe = document.createElement('canvas').getContext('2d', {
    willReadFrequently: true,
  })

  if (!probe) {
    return [0, 0, 0, 1]
  }

  probe.canvas.width = 1
  probe.canvas.height = 1
  probe.fillStyle = cssColor
  probe.fillRect(0, 0, 1, 1)

  const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data

  return [r / 255, g / 255, b / 255, a / 255]
}

use craft_gpu::{GpuContext, GpuFault, GpuJob, GpuJobOutcome, GpuProvider};

use crate::params::parse_u32;

/// CPU stand-in for FilmCraft `transition.wgsl` push/wipe transition (`PushStage` oracle).
///
/// Implements a push-slide transition: at `progress=0` the output is entirely `src` (the
/// incoming layer); at `progress=255` the output is entirely `dst` (the outgoing layer).
/// Both layers slide simultaneously — dst exits in the chosen direction while src enters
/// from the opposite edge — matching the GPU `PushStage` semantics.
///
/// # Job contract
///
/// | Slot | Role |
/// |------|------|
/// | `inputs[0]` | RGBA8 dst/outgoing frame (`w*h*4` bytes) |
/// | `inputs[1]` | RGBA8 src/incoming frame (`w*h*4` bytes) |
///
/// ## Params encoding
///
/// `w:<width>,h:<height>,dir:<direction>,prog:<progress>`
///
/// - `dir` — push direction (0 = right, 1 = left, 2 = down, 3 = up).  
///   "Right" means src enters from the right edge, dst exits to the right.
/// - `prog` — transition progress as an integer 0–255 (0 = full dst, 255 = full src).
pub struct PushTransitionProvider;

impl GpuProvider for PushTransitionProvider {
    fn id(&self) -> &'static str {
        "craft.transition.push"
    }

    fn run(&self, ctx: &mut dyn GpuContext, job: &GpuJob) -> Result<GpuJobOutcome, GpuFault> {
        let width = parse_u32(&job.params, b"w:").unwrap_or(1).max(1) as usize;
        let height = parse_u32(&job.params, b"h:").unwrap_or(1).max(1) as usize;
        let dir = parse_u32(&job.params, b"dir:").unwrap_or(0);
        let prog_int = parse_u32(&job.params, b"prog:").unwrap_or(0).min(255);
        let progress = prog_int as f32 / 255.0;

        let (dst_h, src_h) = match job.inputs.as_slice() {
            [a, b] => (a, b),
            [a] => (a, a),
            _ => return Err(GpuFault::InvalidJob("craft.transition.push expects one or two RGBA8 inputs (dst, src)".into())),
        };

        let dst = ctx.read_buffer(*dst_h)?;
        let src = ctx.read_buffer(*src_h)?;
        let px_count = width.saturating_mul(height);
        let need = px_count.saturating_mul(4);

        if dst.len() < need || src.len() < need {
            return Err(GpuFault::InvalidJob("input buffer shorter than w*h*4".into()));
        }

        let mut out = vec![0u8; need];

        for y in 0..height {
            for x in 0..width {
                let pixel = push_sample(&dst, &src, width, height, x, y, dir, progress);
                let i = (y * width + x) * 4;
                out[i..i + 4].copy_from_slice(&pixel);
            }
        }

        let handle = ctx.upload_buffer("craft.transition.push.out", &out)?;
        Ok(GpuJobOutcome { outputs: vec![handle], data: None })
    }
}

/// Return the output RGBA8 pixel at `(x, y)` for a push-slide transition.
///
/// At `progress == 0` the full dst is shown; at `progress == 1` the full src is shown.
/// Both layers translate at the same speed in the push direction.
fn push_sample(dst: &[u8], src: &[u8], w: usize, h: usize, x: usize, y: usize, dir: u32, progress: f32) -> [u8; 4] {
    // Pixel from a buffer at logical coords (cx, cy), clamped to edge.
    let read = |buf: &[u8], cx: isize, cy: isize| -> [u8; 4] {
        if cx < 0 || cy < 0 || cx >= w as isize || cy >= h as isize {
            return [0, 0, 0, 0];
        }
        let i = (cy as usize * w + cx as usize) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };

    let xi = x as isize;
    let yi = y as isize;
    let wi = w as isize;
    let hi = h as isize;

    // Offset for both layers: dst exits in `dir`, src enters from the opposite side.
    // offset = how many pixels the layers have moved.
    match dir {
        // dir 0: push right — dst exits right, src enters from left.
        0 => {
            let offset = (progress * wi as f32).round() as isize;
            // Try src first (src entered from left): src pixel is at x - offset + w... no.
            // dst occupies [offset, w): output x in [offset, w) shows dst[x - offset]
            // src occupies [0, offset): output x in [0, offset) shows src[w - offset + x]
            if xi < offset {
                // src entering from left
                let src_x = wi - offset + xi;
                read(src, src_x, yi)
            } else {
                // dst sliding right
                read(dst, xi - offset, yi)
            }
        }
        // dir 1: push left — dst exits left, src enters from right.
        1 => {
            let offset = (progress * wi as f32).round() as isize;
            // dst occupies [0, w-offset): output x in [0, w-offset) shows dst[x + offset]
            // src occupies [w-offset, w): output x shows src[x - (w - offset)]
            let split = wi - offset;
            if xi < split {
                read(dst, xi + offset, yi)
            } else {
                read(src, xi - split, yi)
            }
        }
        // dir 2: push down — dst exits bottom, src enters from top.
        2 => {
            let offset = (progress * hi as f32).round() as isize;
            if yi < offset {
                let src_y = hi - offset + yi;
                read(src, xi, src_y)
            } else {
                read(dst, xi, yi - offset)
            }
        }
        // dir 3: push up — dst exits top, src enters from bottom.
        _ => {
            let offset = (progress * hi as f32).round() as isize;
            let split = hi - offset;
            if yi < split {
                read(dst, xi, yi + offset)
            } else {
                read(src, xi, yi - split)
            }
        }
    }
}

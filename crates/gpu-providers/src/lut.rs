use craft_gpu::{GpuContext, GpuFault, GpuJob, GpuJobOutcome, GpuProvider};

use crate::params::parse_u32;

/// CPU stand-in for FilmCraft `lut.wgsl` tetrahedral 3D LUT (`filmcraft_color::Lut3d` oracle).
///
/// # Job contract
///
/// | Slot | Role |
/// |------|------|
/// | `inputs[0]` | RGBA8 source image (`w*h*4` bytes) |
/// | `inputs[1]` | 3D LUT table: `n^3` RGB f32 triplets, little-endian, B-major ordering |
///
/// ## Params encoding
///
/// `w:<width>,h:<height>,n:<lut_size>`
///
/// - `n` is the number of samples per axis (e.g. 17, 33, 65).
///   Buffer length must be `n*n*n*12` bytes.
/// - Alpha channel is preserved unchanged.
pub struct LutProvider;

impl GpuProvider for LutProvider {
    fn id(&self) -> &'static str {
        "craft.lut"
    }

    fn run(&self, ctx: &mut dyn GpuContext, job: &GpuJob) -> Result<GpuJobOutcome, GpuFault> {
        let width = parse_u32(&job.params, b"w:").unwrap_or(1).max(1) as usize;
        let height = parse_u32(&job.params, b"h:").unwrap_or(1).max(1) as usize;
        let n = parse_u32(&job.params, b"n:").ok_or_else(|| GpuFault::InvalidJob("craft.lut: missing param n:".into()))? as usize;
        if n < 2 {
            return Err(GpuFault::InvalidJob("craft.lut: n must be >= 2".into()));
        }

        let (img_h, lut_h) = match job.inputs.as_slice() {
            [a, b] => (a, b),
            _ => return Err(GpuFault::InvalidJob("craft.lut expects two inputs: image and LUT table".into())),
        };

        let img = ctx.read_buffer(*img_h)?;
        let lut_bytes = ctx.read_buffer(*lut_h)?;

        let px_count = width.saturating_mul(height);
        let img_need = px_count.saturating_mul(4);
        let lut_need = n.saturating_mul(n).saturating_mul(n).saturating_mul(12);

        if img.len() < img_need {
            return Err(GpuFault::InvalidJob(format!("craft.lut: image buffer too small ({} < {img_need})", img.len())));
        }
        if lut_bytes.len() < lut_need {
            return Err(GpuFault::InvalidJob(format!("craft.lut: LUT buffer too small ({} < {lut_need})", lut_bytes.len())));
        }

        let lut = parse_lut_f32(&lut_bytes[..lut_need], n);
        let mut out = vec![0u8; img_need];

        for px in 0..px_count {
            let i = px * 4;
            let r_in = f32::from(img[i]) / 255.0;
            let g_in = f32::from(img[i + 1]) / 255.0;
            let b_in = f32::from(img[i + 2]) / 255.0;
            let a = img[i + 3];

            let [r_out, g_out, b_out] = tetrahedral_interp(&lut, n, r_in, g_in, b_in);

            out[i] = (r_out.clamp(0.0, 1.0) * 255.0).round() as u8;
            out[i + 1] = (g_out.clamp(0.0, 1.0) * 255.0).round() as u8;
            out[i + 2] = (b_out.clamp(0.0, 1.0) * 255.0).round() as u8;
            out[i + 3] = a;
        }

        let handle = ctx.upload_buffer("craft.lut.out", &out)?;
        Ok(GpuJobOutcome { outputs: vec![handle], data: None })
    }
}

/// Parse `n^3` RGB f32 triplets (little-endian) from raw bytes.
/// Returns a flat `Vec<[f32; 3]>` indexed by `r*n*n + g*n + b` (B changes fastest).
fn parse_lut_f32(bytes: &[u8], n: usize) -> Vec<[f32; 3]> {
    let count = n * n * n;
    let mut lut = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * 12;
        let r = f32::from_le_bytes(bytes[base..base + 4].try_into().unwrap_or([0; 4]));
        let g = f32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap_or([0; 4]));
        let b = f32::from_le_bytes(bytes[base + 8..base + 12].try_into().unwrap_or([0; 4]));
        lut.push([r, g, b]);
    }
    lut
}

/// Index into `lut` with B-major ordering: `lut[r*n*n + g*n + b]`.
fn lut_at(lut: &[[f32; 3]], n: usize, ri: usize, gi: usize, bi: usize) -> [f32; 3] {
    let idx = ri * n * n + gi * n + bi;
    if idx < lut.len() { lut[idx] } else { [0.0; 3] }
}

/// Tetrahedral interpolation — matches the standard algorithm used in LUT shaders
/// (Sakamoto / Colour Science reference, also matches `filmcraft_color::Lut3d`).
fn tetrahedral_interp(lut: &[[f32; 3]], n: usize, r: f32, g: f32, b: f32) -> [f32; 3] {
    let scale = (n - 1) as f32;
    let rf = (r * scale).clamp(0.0, scale);
    let gf = (g * scale).clamp(0.0, scale);
    let bf = (b * scale).clamp(0.0, scale);

    let r0 = rf.floor() as usize;
    let g0 = gf.floor() as usize;
    let b0 = bf.floor() as usize;
    let r1 = (r0 + 1).min(n - 1);
    let g1 = (g0 + 1).min(n - 1);
    let b1 = (b0 + 1).min(n - 1);

    let dr = rf - r0 as f32;
    let dg = gf - g0 as f32;
    let db = bf - b0 as f32;

    // Six tetrahedra covering the unit cube (Sakamoto ordering).
    let c000 = lut_at(lut, n, r0, g0, b0);
    let c100 = lut_at(lut, n, r1, g0, b0);
    let c010 = lut_at(lut, n, r0, g1, b0);
    let c001 = lut_at(lut, n, r0, g0, b1);
    let c110 = lut_at(lut, n, r1, g1, b0);
    let c011 = lut_at(lut, n, r0, g1, b1);
    let c101 = lut_at(lut, n, r1, g0, b1);
    let c111 = lut_at(lut, n, r1, g1, b1);

    let mut out = [0.0f32; 3];
    for ch in 0..3 {
        let v = if dr >= dg && dg >= db {
            // R >= G >= B
            (1.0 - dr) * c000[ch] + (dr - dg) * c100[ch] + (dg - db) * c110[ch] + db * c111[ch]
        } else if dr >= db && db >= dg {
            // R >= B >= G
            (1.0 - dr) * c000[ch] + (dr - db) * c100[ch] + (db - dg) * c101[ch] + dg * c111[ch]
        } else if db >= dr && dr >= dg {
            // B >= R >= G
            (1.0 - db) * c000[ch] + (db - dr) * c001[ch] + (dr - dg) * c101[ch] + dg * c111[ch]
        } else if dg >= dr && dr >= db {
            // G >= R >= B
            (1.0 - dg) * c000[ch] + (dg - dr) * c010[ch] + (dr - db) * c110[ch] + db * c111[ch]
        } else if dg >= db && db >= dr {
            // G >= B >= R
            (1.0 - dg) * c000[ch] + (dg - db) * c010[ch] + (db - dr) * c011[ch] + dr * c111[ch]
        } else {
            // B >= G >= R
            (1.0 - db) * c000[ch] + (db - dg) * c001[ch] + (dg - dr) * c011[ch] + dr * c111[ch]
        };
        out[ch] = v;
    }
    out
}

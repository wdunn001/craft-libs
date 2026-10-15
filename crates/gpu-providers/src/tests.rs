#![allow(clippy::expect_used)]

use craft_gpu::{GpuContext, GpuJob, GpuJobParams, ProviderRegistry, WorkerKind, f16_to_f32, f16_to_f32_fast};
use craft_gpu_null::NullContext;

use crate::{EchoProvider, LutProvider, MaskProvider, NoopProvider, PremulCompositeProvider, PushTransitionProvider, ReadbackProvider, register_all, register_all_into};

#[test]
fn noop_provider_registered_by_id() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(NoopProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    let job = GpuJob { provider_id: "craft.noop".into(), params: GpuJobParams(Vec::new()), inputs: vec![], outputs: vec![] };
    let out = registry.dispatch(&mut ctx, &job).expect("noop dispatch");
    assert!(out.data.is_none());
}

#[test]
fn echo_provider_concatenates_params_and_first_input() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(EchoProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    let h = ctx.upload_buffer("in", b"xy").expect("upload_buffer in test");
    let job = GpuJob { provider_id: "craft.test.echo".into(), params: GpuJobParams(b"a".to_vec()), inputs: vec![h], outputs: vec![] };
    let out = registry.dispatch(&mut ctx, &job).expect("echo dispatch");
    assert_eq!(out.data.as_deref(), Some(b"axy".as_slice()));
}

#[test]
fn register_all_lists_shared_providers() {
    let ids: Vec<_> = register_all().iter().map(|p| p.id()).collect();
    assert_eq!(
        ids,
        vec!["craft.noop", "craft.test.echo", "craft.mask", "craft.composite.premul", "craft.readback", "craft.lut", "craft.transition.push",]
    );
}

#[test]
fn gpu_providers_cargo_has_no_app_deps() {
    let manifest = include_str!("../Cargo.toml");
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with("effectcraft") || t.starts_with("filmcraft") || t.contains("effectcraft-") || t.contains("filmcraft-") {
            panic!("forbidden app dependency line: {t}");
        }
    }
}

#[test]
fn mask_provider_multiplies_rgba_by_mask_on_cpu() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MaskProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    let base = ctx.upload_buffer("base", &[255u8, 0, 0, 255, 100, 100, 100, 255]).expect("upload base");
    let mask = ctx.upload_buffer("mask", &[128u8, 255, 255, 255, 0, 255, 255, 255]).expect("upload mask");
    let job =
        craft_gpu::GpuJob { provider_id: "craft.mask".into(), params: GpuJobParams(b"w:1,h:2".to_vec()), inputs: vec![base, mask], outputs: vec![] };
    let out = registry.dispatch(&mut ctx, &job).expect("mask dispatch");
    let handle = out.outputs[0];
    let bytes = ctx.read_buffer(handle).expect("read mask out");
    assert_eq!(bytes[0], 128);
    assert_eq!(bytes[4], 0);
}

#[test]
fn premul_composite_over_on_cpu() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(PremulCompositeProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    // backdrop: green opaque; layer: half-alpha blue
    let dst = ctx.upload_buffer("dst", &[0, 255, 0, 255]).expect("upload dst");
    let src = ctx.upload_buffer("src", &[0, 0, 255, 128]).expect("upload src");
    let job =
        GpuJob { provider_id: "craft.composite.premul".into(), params: GpuJobParams(b"w:1,h:1".to_vec()), inputs: vec![dst, src], outputs: vec![] };
    let out = registry.dispatch(&mut ctx, &job).expect("premul composite");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");
    // backdrop green(0,255,0,255) + half-alpha blue(0,0,255,128) premul over.
    // green channel: 255*(1 - 128/255) = 127.0; blue: 255*(128/255) = 128.
    assert_eq!(bytes[0], 0);
    assert_eq!(bytes[1], 127); // green: 255*(1-128/255) rounds to 127
    assert_eq!(bytes[2], 128); // blue: 255*(128/255) = 128
    assert_eq!(bytes[3], 255);
}

#[test]
fn readback_decodes_f16_rgba_to_f32() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(ReadbackProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    let h = half_float_one();
    let mut raw = Vec::new();
    raw.extend_from_slice(&h.to_le_bytes());
    raw.extend_from_slice(&h.to_le_bytes());
    raw.extend_from_slice(&h.to_le_bytes());
    raw.extend_from_slice(&0u16.to_le_bytes());
    let inp = ctx.upload_buffer("accum", &raw).expect("upload f16");
    let job = GpuJob { provider_id: "craft.readback".into(), params: GpuJobParams(b"w:1,h:1".to_vec()), inputs: vec![inp], outputs: vec![] };
    let out = registry.dispatch(&mut ctx, &job).expect("readback");
    let decoded = ctx.read_buffer(out.outputs[0]).expect("read f32 out");
    assert_eq!(decoded.len(), 16);
    let r = f32::from_le_bytes(decoded[0..4].try_into().expect("r"));
    assert!((r - 1.0).abs() < 1e-3);
    assert_eq!(f16_to_f32(h), f16_to_f32_fast(h));
}

fn half_float_one() -> u16 {
    f32_to_f16(1.0)
}

fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 31) & 1) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x007f_ffff;
    if exp <= 0 {
        return sign << 15;
    }
    if exp >= 0x1f {
        return (sign << 15) | 0x7c00;
    }
    (sign << 15) | ((exp as u16) << 10) | ((mant >> 13) as u16)
}

#[test]
fn register_all_into_populates_registry() {
    let mut registry = ProviderRegistry::new();
    register_all_into(&mut registry);
    let mut ctx = NullContext::new(WorkerKind::SharedWorker);
    let job = GpuJob { provider_id: "craft.noop".into(), params: GpuJobParams(Vec::new()), inputs: vec![], outputs: vec![] };
    registry.dispatch(&mut ctx, &job).expect("noop via register_all_into");
}

// --- craft.lut golden tests ---

/// Build a 2-node identity 3D LUT: each of the 8 corners maps output == input.
/// With n=2: entries for (r,g,b) in {0,1}^3, value = (r_f, g_f, b_f).
fn identity_lut_2() -> Vec<u8> {
    // B-major ordering: for r in 0..2, g in 0..2, b in 0..2
    let mut bytes = Vec::with_capacity(8 * 12);
    for r in 0u8..2 {
        for g in 0u8..2 {
            for b in 0u8..2 {
                bytes.extend_from_slice(&(r as f32).to_le_bytes());
                bytes.extend_from_slice(&(g as f32).to_le_bytes());
                bytes.extend_from_slice(&(b as f32).to_le_bytes());
            }
        }
    }
    bytes
}

#[test]
fn lut_identity_passes_pixels_through() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(LutProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // Single pixel: full red (255, 0, 0, 255).
    let img = ctx.upload_buffer("img", &[255u8, 0, 0, 255]).expect("upload img");
    let lut_bytes = identity_lut_2();
    let lut = ctx.upload_buffer("lut", &lut_bytes).expect("upload lut");

    let job = GpuJob {
        provider_id: "craft.lut".into(),
        params: GpuJobParams(b"w:1,h:1,n:2".to_vec()),
        inputs: vec![img, lut],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("lut dispatch");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read lut out");
    // Identity LUT must preserve input (within 1 LSB rounding).
    assert!((bytes[0] as i32 - 255).abs() <= 1, "R pass-through: got {}", bytes[0]);
    assert!((bytes[1] as i32 - 0).abs() <= 1, "G pass-through: got {}", bytes[1]);
    assert!((bytes[2] as i32 - 0).abs() <= 1, "B pass-through: got {}", bytes[2]);
    assert_eq!(bytes[3], 255, "alpha preserved");
}

#[test]
fn lut_alpha_preserved_and_not_transformed() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(LutProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // Semi-transparent pixel: (0, 255, 0, 128).
    let img = ctx.upload_buffer("img", &[0u8, 255, 0, 128]).expect("upload img");
    let lut_bytes = identity_lut_2();
    let lut = ctx.upload_buffer("lut", &lut_bytes).expect("upload lut");

    let job = GpuJob {
        provider_id: "craft.lut".into(),
        params: GpuJobParams(b"w:1,h:1,n:2".to_vec()),
        inputs: vec![img, lut],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("lut dispatch");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");
    assert_eq!(bytes[3], 128, "alpha must be copied unchanged");
}

/// Build a 3-node constant LUT that maps every input to (128, 64, 192).
fn constant_lut_3(r_val: f32, g_val: f32, b_val: f32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(27 * 12);
    for _ in 0..27 {
        bytes.extend_from_slice(&r_val.to_le_bytes());
        bytes.extend_from_slice(&g_val.to_le_bytes());
        bytes.extend_from_slice(&b_val.to_le_bytes());
    }
    bytes
}

#[test]
fn lut_constant_maps_all_pixels_to_fixed_color() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(LutProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // Any 2 pixels; constant LUT maps all to (0.5, 0.25, 0.75) ~ (128, 64, 191).
    let img = ctx.upload_buffer("img", &[10u8, 200, 50, 255, 80, 30, 180, 200]).expect("upload img");
    let lut_bytes = constant_lut_3(0.5, 0.25, 0.75);
    let lut = ctx.upload_buffer("lut", &lut_bytes).expect("upload lut");

    let job = GpuJob {
        provider_id: "craft.lut".into(),
        params: GpuJobParams(b"w:2,h:1,n:3".to_vec()),
        inputs: vec![img, lut],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("lut dispatch");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read lut out");

    // Both pixels must map to ~(128, 64, 191) — constant LUT, interpolation has no effect.
    for px in [0, 1] {
        let i = px * 4;
        assert!((bytes[i] as i32 - 128).abs() <= 1, "px{px} R: got {}", bytes[i]);
        assert!((bytes[i + 1] as i32 - 64).abs() <= 1, "px{px} G: got {}", bytes[i + 1]);
        assert!((bytes[i + 2] as i32 - 191).abs() <= 2, "px{px} B: got {}", bytes[i + 2]);
    }
}

#[test]
fn lut_rejects_missing_n_param() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(LutProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);
    let img = ctx.upload_buffer("img", &[0u8; 4]).expect("upload img");
    let lut = ctx.upload_buffer("lut", &[0u8; 96]).expect("upload lut");
    let job = GpuJob {
        provider_id: "craft.lut".into(),
        params: GpuJobParams(b"w:1,h:1".to_vec()), // missing n:
        inputs: vec![img, lut],
        outputs: vec![],
    };
    assert!(registry.dispatch(&mut ctx, &job).is_err(), "should error without n:");
}

// --- craft.transition.push golden tests ---

#[test]
fn push_transition_at_zero_shows_full_dst() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(PushTransitionProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // dst = red pixel, src = blue pixel; progress=0 => full dst.
    let dst = ctx.upload_buffer("dst", &[255u8, 0, 0, 255]).expect("upload dst");
    let src = ctx.upload_buffer("src", &[0u8, 0, 255, 255]).expect("upload src");
    let job = GpuJob {
        provider_id: "craft.transition.push".into(),
        params: GpuJobParams(b"w:1,h:1,dir:0,prog:0".to_vec()),
        inputs: vec![dst, src],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("push transition");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");
    assert_eq!(bytes[0], 255, "R should be dst red at prog=0");
    assert_eq!(bytes[2], 0, "B should be 0 (dst) at prog=0");
}

#[test]
fn push_transition_at_full_shows_full_src() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(PushTransitionProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // dst = red, src = blue; progress=255 => full src.
    let dst = ctx.upload_buffer("dst", &[255u8, 0, 0, 255]).expect("upload dst");
    let src = ctx.upload_buffer("src", &[0u8, 0, 255, 255]).expect("upload src");
    let job = GpuJob {
        provider_id: "craft.transition.push".into(),
        params: GpuJobParams(b"w:1,h:1,dir:0,prog:255".to_vec()),
        inputs: vec![dst, src],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("push transition");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");
    assert_eq!(bytes[2], 255, "B should be src blue at prog=255");
    assert_eq!(bytes[0], 0, "R should be 0 (src) at prog=255");
}

#[test]
fn push_transition_dir_right_splits_horizontally() {
    // 2x1 image, dir=0 (push right), prog=128 (~50%).
    // Left pixel should be src (entering from left), right pixel should be dst (sliding right).
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(PushTransitionProvider));
    let mut ctx = NullContext::new(WorkerKind::Main);

    // dst = [red, red], src = [blue, blue]
    let dst = ctx.upload_buffer("dst", &[255u8, 0, 0, 255, 255, 0, 0, 255]).expect("upload dst");
    let src = ctx.upload_buffer("src", &[0u8, 0, 255, 255, 0, 0, 255, 255]).expect("upload src");
    let job = GpuJob {
        provider_id: "craft.transition.push".into(),
        params: GpuJobParams(b"w:2,h:1,dir:0,prog:128".to_vec()),
        inputs: vec![dst, src],
        outputs: vec![],
    };
    let out = registry.dispatch(&mut ctx, &job).expect("push transition");
    let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");

    // At ~50%: left pixel (x=0) should be src (blue), right pixel (x=1) should be dst (red).
    assert_eq!(bytes[2], 255, "x=0 should be src (blue B=255)");
    assert_eq!(bytes[4], 255, "x=1 should be dst (red R=255)");
}

#[test]
fn push_transition_all_dirs_produce_correct_length() {
    for dir in 0u32..4 {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(PushTransitionProvider));
        let mut ctx = NullContext::new(WorkerKind::Main);
        let dst = ctx.upload_buffer("dst", &[0u8; 16]).expect("upload dst");
        let src = ctx.upload_buffer("src", &[0u8; 16]).expect("upload src");
        let params = format!("w:2,h:2,dir:{dir},prog:128");
        let job = GpuJob {
            provider_id: "craft.transition.push".into(),
            params: GpuJobParams(params.into_bytes()),
            inputs: vec![dst, src],
            outputs: vec![],
        };
        let out = registry.dispatch(&mut ctx, &job).expect(&format!("dir={dir}"));
        let bytes = ctx.read_buffer(out.outputs[0]).expect("read out");
        assert_eq!(bytes.len(), 16, "dir={dir}: output must be w*h*4");
    }
}

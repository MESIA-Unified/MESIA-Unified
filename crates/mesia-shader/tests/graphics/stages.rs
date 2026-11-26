//! Original, self-contained GFX10 vertex and pixel programs for the moving-quad fixture.
//! Offsets in the stage records follow the layout documented in src/native.rs.

use mesia_shader::HwStage;
use mesia_shader::emit::{HostShader, translate};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// A signature with one float4 in row 0 of stream 0.
fn signature(name: &str, system_value: u32, input: bool) -> Vec<u8> {
    let mut part = vec![0; 40];
    put32(&mut part, 0, 1);
    put32(&mut part, 4, 8);
    put32(&mut part, 12, 40);
    put32(&mut part, 20, system_value);
    put32(&mut part, 24, 3); // float32
    part[32] = 0xf;
    part[33] = if input { 0xf } else { 0 };
    part.extend(name.bytes());
    part.push(0);
    part
}

fn container(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0; 32 + 4 * parts.len()];
    out[..4].copy_from_slice(b"DXBC");
    put32(&mut out, 20, 1);
    put32(&mut out, 28, parts.len() as u32);
    for (i, (tag, part)) in parts.iter().enumerate() {
        out.resize(out.len().next_multiple_of(4), 0);
        let offset = out.len() as u32;
        put32(&mut out, 32 + 4 * i, offset);
        out.extend_from_slice(*tag);
        out.extend((part.len() as u32).to_le_bytes());
        out.extend(part);
    }
    let length = out.len() as u32;
    put32(&mut out, 24, length);
    out
}

fn native_part(slot: usize, stage_data: &[u8], program: &[u32]) -> Vec<u8> {
    const RECORD: usize = 0x68;
    const RECORD_SIZE: usize = 0x54;
    let mut payload = vec![0; RECORD + RECORD_SIZE];
    put64(&mut payload, 0, 6);
    for other in [8, 16, 24, 32, 40, 48, 72, 96] {
        put64(&mut payload, other, 0xdead_beef_dead_beef);
    }
    put64(&mut payload, slot, RECORD as u64);
    put64(
        &mut payload,
        RECORD,
        (RECORD + RECORD_SIZE + stage_data.len()) as u64,
    );
    put64(&mut payload, RECORD + 8, (program.len() * 4) as u64);
    put32(&mut payload, RECORD + 0x14, 1 << 5); // wave32
    put32(&mut payload, RECORD + 0x50, 8); // highest VGPR read is v7
    payload.extend(stage_data);
    for word in program {
        payload.extend(word.to_le_bytes());
    }
    let mut fssm = b"FSSM".to_vec();
    fssm.extend(0x101u32.to_le_bytes());
    fssm.extend((payload.len() as u64).to_le_bytes());
    fssm.extend(payload);
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(&fssm, 6);
    let mut xbch = b"XBCH".to_vec();
    xbch.extend(3u32.to_le_bytes());
    xbch.extend((packed.len() as u64).to_le_bytes());
    xbch.extend((fssm.len() as u64).to_le_bytes());
    xbch.extend(packed);
    let mut part = vec![0; 0x30];
    part[0x18..0x1c].copy_from_slice(b"XDAT");
    put32(&mut part, 0x1c, 1);
    put32(&mut part, 0x20, 0x24);
    put32(&mut part, 0x24, 13); // native program entry
    put32(&mut part, 0x28, 0x30);
    put32(&mut part, 0x2c, xbch.len() as u32);
    part.extend(xbch);
    part
}

/// POSITION0 is a dynamic, tightly packed float4 in clip space. The pixel
/// shader exports opaque magenta to SV_Target0 for every covered sample.
pub fn translated_stages() -> (HostShader, HostShader) {
    let mut vs_metadata = vec![0; 0x18c];
    vs_metadata[0x24..0x29].copy_from_slice(&[6, 0, 0xf, 4, 3]); // row 0 -> v4..v7
    put32(&mut vs_metadata, 0xc4, 1);
    let vs = native_part(
        0x28,
        &vs_metadata,
        &[
            0xf800_08cf,
            0x0706_0504, // exp pos0 v4,v5,v6,v7 done
            0xbf81_0000, // s_endpgm
        ],
    );
    let vs = translate(&container(&[
        (b"DXIL", vs),
        (b"ISG1", signature("POSITION", 0, true)),
        (b"OSG1", signature("SV_Position", 1, false)),
    ]))
    .expect("translate original position export");
    const PATCH_TABLE: usize = 0x14c;
    const PATCH_BYTES: usize = 4 + 10 * 10 * 4;
    let mut ps_metadata = vec![0; PATCH_TABLE + 8 * PATCH_BYTES];
    for mrt in 0..8 {
        put32(&mut ps_metadata, PATCH_TABLE + mrt * PATCH_BYTES, u32::MAX);
    }
    let ps = native_part(
        0x30,
        &ps_metadata,
        &[
            0x7e00_02f2, // v_mov_b32 v0, 1.0
            0x7e02_0280, // v_mov_b32 v1, 0
            0xf800_180f,
            0x0000_0100, // exp mrt0 v0,v1,v0,v0 done vm
            0xbf81_0000, // s_endpgm
        ],
    );
    let ps = translate(&container(&[
        (b"DXIL", ps),
        (b"OSG1", signature("SV_Target", 64, false)),
    ]))
    .expect("translate original colour export");
    assert_eq!(vs.stage, HwStage::Vs);
    assert_eq!(ps.stage, HwStage::Ps);
    (vs, ps)
}

/// Validate copies, so the PSO receives exactly the bytes emitted by MESIA.
pub fn validate_stages(vs: &HostShader, ps: &HostShader) {
    use mesia_shader::emit::validator::Validator;

    let mut validator = Validator::load(Validator::default_path().as_deref())
        .expect("the graphics baseline requires the Windows SDK DXIL validator");
    for stage in [vs, ps] {
        let mut copy = stage.dxil.clone();
        validator.validate(&mut copy).unwrap_or_else(|error| {
            panic!("DXIL validation failed for {:?}: {error}", stage.stage)
        });
        assert_eq!(copy[4..20], stage.dxil[4..20], "DXIL container digest");
    }
}

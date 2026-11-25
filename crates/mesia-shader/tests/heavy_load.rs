//! Heavy, deterministic synthetic render load: the reference workload the
//! project's per-frame perf tiers (333 / 200 / 166 / 125 / 100 ms) are
//! measured against. One frame carries the submission mass of a title
//! frame - about 6,000 draws and dispatches over four passes, the profile
//! of the ~585 ms frame this load stands in for:
//!
//!   pass 1 "gbuffer"   2800 instanced draws of 32 scattered quads each
//!                      (32..288 px, integer pixel edges) into MRT x2 + a
//!                      D32 depth buffer, ~300 texel loads a pixel;
//!   pass 2 "forward"   2200 instanced draws of 32 smaller quads each
//!                      (24..224 px) into a third target, same depth;
//!   pass 3 "composite" 540 tile draws of 64x60 px that read all three
//!                      targets back plus the big textures into the final
//!                      target;
//!   pass 4 "compute"   512 dispatches of (8,4) groups of 8x8 threads,
//!                      8 scattered loads + a 48k-round hash each,
//!                      writing a 1024x1024 UAV.
//!
//! `MESIA_HEAVY_DRAWS` multiplies the four pass counts (e.g. 0.5 or 2),
//! growing or shrinking the same scene around the tiers. Everything is
//! procedural: positions, sizes, depths and colours are pure hashes of
//! one fixed seed - the shaders hash per-instance on the GPU, the harness
//! hashes per-draw and per-texel on the CPU - and the rendered image is
//! the same in every frame of every run, so the readback's FNV-1a depends
//! on the draw counts only, not on the frame index or count. Every
//! primitive edge lies on an integer pixel boundary and every division is
//! by a power of two, so the rasterised content is exact.
//!
//! Timing: `cpu` is the submit thread's record + ExecuteCommandLists;
//! `gpu` is the queue's own timeline read from TIMESTAMP queries (three
//! per frame: submit start, after the compute pass, end), so the render
//! work and the present-stand-in target copy read apart. Frames run
//! `IN_FLIGHT` deep like a real presenter; `qwait` is what waiting for a
//! free submission slot costs the submit thread. `MESIA_HEAVY_FRAMES`
//! sets the measured frames (default 120); four warmup frames first.
//!
//! Skipped without `MESIA_HEAVY=1`, a PC dxc, or a device (`MESIA_WARP=1`
//! forces WARP). The last frame's target is hashed and printed - an
//! observed value to record, not a golden.

#![cfg(all(windows, feature = "host"))]

use std::path::{Path, PathBuf};
use std::time::Instant;

use mesia_shader::emit::host::Host;
use mesia_shader::emit::validator::Validator;
use mesia_shader::emit::{HostShader, UserData};
use mesia_shader::{HwStage, RootType};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{Interface, PCSTR};

/// The scene's HLSL: one file, compiled per entry like `synth_scene`'s.
/// Every stage reads the same sixteen-dword `cbuffer Ud` at `b0, space0`
/// (the host root signature's per-stage root constants), and resources
/// are heap-indexed exactly as translated shaders do: the four big
/// textures at heap descriptors 0..4, the three render targets at 4..7
/// when they are shader resources, and the compute UAV at 7.
const HLSL: &str = r#"
cbuffer Ud : register(b0, space0) {
    uint4 ud0;
    uint4 ud1;
    uint4 ud2;
    uint4 ud3;
};

uint hmix(uint x) {
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

struct Quad {
    float4 pos : SV_Position;
    nointerpolation uint4 seed : TEXCOORD0;
};

// Pass 1 vertex: one scattered quad per instance; position, size and
// depth all come from the (draw seed, instance id) hash, on integer
// pixel edges so coverage is exact.
Quad vs_gb(float2 corner : POSITION, uint iid : SV_InstanceID) {
    uint h0 = hmix(ud0.x + iid * 0x9e3779b9u);
    uint h1 = hmix(h0 ^ ud0.y);
    uint h2 = hmix(h1 + iid);
    float sz = 32.0 + float(h2 % 257u);
    float2 px = float2(float(h0 % 1920u), float(h1 % 1080u)) + corner * sz;
    float depth = float(h0 % 1024u) * 0.0009765625;
    Quad o;
    o.pos = float4(px.x / 960.0 - 1.0, 1.0 - px.y / 540.0, depth, 1.0);
    o.seed = uint4(h0, h1, h2, iid);
    return o;
}

struct Two {
    float4 a : SV_Target0;
    float4 b : SV_Target1;
};

// Pass 1 pixel: four base taps then ninety-six rounds of three scattered
// 2048x2048 loads; ~300 texel fetches per pixel of overdraw.
Two ps_gb(Quad i) {
    Texture2D<float4> t0 = ResourceDescriptorHeap[0];
    Texture2D<float4> t1 = ResourceDescriptorHeap[1];
    Texture2D<float4> t2 = ResourceDescriptorHeap[2];
    Texture2D<float4> t3 = ResourceDescriptorHeap[3];
    uint2 p = uint2(i.pos.xy);
    uint2 c = (p + i.seed.xy) & 2047u;
    float4 a = t0.Load(int3(c, 0)) + t1.Load(int3((c + i.seed.zw) & 2047u, 0));
    float4 b = t2.Load(int3((c * 3u + i.seed.yw) & 2047u, 0))
             + t3.Load(int3((c.yx ^ i.seed.xx) & 2047u, 0));
    float4 acc = a * 0.5 + b * 0.25;
    for (uint k = 0u; k < 96u; ++k) {
        uint2 cc = (c + uint2(k * 37u, k * 91u) + (i.seed.xz >> (k & 15u))) & 2047u;
        acc = acc * 0.96875 + t0.Load(int3(cc, 0)) * 0.03125;
        acc += t1.Load(int3(cc ^ 0x05a5u, 0)) * 0.03125
             - t2.Load(int3((cc + k) & 2047u, 0)) * 0.015625;
    }
    Two o;
    o.a = saturate(acc);
    o.b = saturate(acc.wzyx * 0.5 + a * 0.125);
    return o;
}

// Pass 2 vertex: the same scatter, another salt, smaller quads.
Quad vs_fw(float2 corner : POSITION, uint iid : SV_InstanceID) {
    uint h0 = hmix(ud0.x + iid * 0x85ebca6bu);
    uint h1 = hmix(h0 + 0x27d4eb2fu);
    uint h2 = hmix(h1 ^ iid);
    float sz = 24.0 + float(h2 % 201u);
    float2 px = float2(float(h0 % 1920u), float(h1 % 1080u)) + corner * sz;
    float depth = float(h0 % 1024u) * 0.0009765625;
    Quad o;
    o.pos = float4(px.x / 960.0 - 1.0, 1.0 - px.y / 540.0, depth, 1.0);
    o.seed = uint4(h0, h1, h2, iid);
    return o;
}

// Pass 2 pixel: a base pair plus ten scattered taps.
float4 ps_fw(Quad i) : SV_Target {
    Texture2D<float4> t0 = ResourceDescriptorHeap[0];
    Texture2D<float4> t1 = ResourceDescriptorHeap[1];
    Texture2D<float4> t3 = ResourceDescriptorHeap[3];
    uint2 p = uint2(i.pos.xy);
    uint2 c = (p * 2u + i.seed.yx) & 2047u;
    float4 acc = t0.Load(int3(c, 0)) * 0.5 + t1.Load(int3((c + i.seed.zw) & 2047u, 0));
    for (uint k = 0u; k < 32u; ++k) {
        uint2 cc = (c + uint2(k * 53u, k * 29u) + i.seed.wz) & 2047u;
        acc = acc * 0.9375 + t3.Load(int3(cc, 0)) * 0.0625;
    }
    return saturate(acc);
}

// Pass 3 vertex: a 64x60 px composite tile; ud0.xy is its pixel origin.
Quad vs_tile(float2 corner : POSITION) {
    float2 px = float2(ud0.x, ud0.y) + corner * float2(64.0, 60.0);
    Quad o;
    o.pos = float4(px.x / 960.0 - 1.0, 1.0 - px.y / 540.0, 0.5, 1.0);
    o.seed = uint4(ud0.x, ud0.y, ud0.z, 0u);
    return o;
}

// Pass 3 pixel: the forward target plus a strided 5x5 of the first
// gbuffer target, the second target as-is, and thirty-two big-texture
// taps.
float4 ps_tile(Quad i) : SV_Target {
    Texture2D<float4> ga = ResourceDescriptorHeap[4];
    Texture2D<float4> gb = ResourceDescriptorHeap[5];
    Texture2D<float4> fw = ResourceDescriptorHeap[6];
    Texture2D<float4> t0 = ResourceDescriptorHeap[0];
    Texture2D<float4> t3 = ResourceDescriptorHeap[3];
    int2 p = int2(i.pos.xy);
    float4 acc = fw.Load(int3(p, 0)) + gb.Load(int3(p, 0)) * 0.5;
    for (int dy = -2; dy <= 2; ++dy) {
        for (int dx = -2; dx <= 2; ++dx) {
            int2 q = clamp(p + int2(dx * 3, dy * 3), int2(0, 0), int2(1919, 1079));
            acc += ga.Load(int3(q, 0)) * 0.04;
        }
    }
    uint2 c = (uint2(p) + i.seed.xy) & 2047u;
    for (uint k = 0u; k < 16u; ++k) {
        uint2 cc = (c + uint2(k * 251u, k * 131u)) & 2047u;
        acc += t0.Load(int3(cc, 0)) * 0.0625 + t3.Load(int3(cc ^ 0x03c3u, 0)) * 0.03125;
    }
    return saturate(acc * 0.5);
}

// Pass 4: eight scattered loads then a 49152-round integer hash per
// thread of the 1024x1024 UAV; ud0.xy is the dispatch's tile origin and
// ud0.z its seed.
[numthreads(8, 8, 1)]
void cs_work(uint3 tid : SV_DispatchThreadID) {
    RWTexture2D<float4> o = ResourceDescriptorHeap[7];
    Texture2D<float4> t1 = ResourceDescriptorHeap[1];
    uint2 px = uint2(ud0.x, ud0.y) + tid.xy;
    uint v = hmix(px.x + px.y * 1024u + ud0.z);
    float4 acc = float4(0.0, 0.0, 0.0, 0.0);
    for (uint k = 0u; k < 8u; ++k) {
        v = hmix(v);
        acc += t1.Load(int3((px + uint2(v & 511u, (v >> 9u) & 511u)) & 2047u, 0)) * 0.125;
    }
    for (uint k = 0u; k < 49152u; ++k) {
        v = v * 1664525u + 1013904223u;
        v ^= v >> 15u;
    }
    float f = float(v & 255u) * 0.00390625;
    o[px] = float4(f + acc.r * 0.5, acc.g * 0.5, f, 1.0);
}
"#;

const W: u32 = 1920;
const H: u32 = 1080;
const TEX: u32 = 2048;
const N_TEX: usize = 4;
const UAV_SIDE: u32 = 1024;
const FMT8: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const DEPTH_FMT: DXGI_FORMAT = DXGI_FORMAT_D32_FLOAT;
/// The scene's fixed seed: draw seeds and texels all derive from it.
const SEED: u32 = 0x5eed_c0de;
/// Root-constant dwords each stage declares (`cbuffer Ud`).
const UD_DWORDS: u32 = 16;
/// Submission slots: how far ahead of the GPU the submit thread runs.
const IN_FLIGHT: usize = 8;
/// Frames submitted before the measured ones (pipeline/shader warm-up).
const WARMUP: usize = 4;
/// Submissions a frame per pass at `MESIA_HEAVY_DRAWS`=1.0: gbuffer,
/// forward, composite tiles, compute dispatches - about 6k together.
const PASSES: [u32; 4] = [2800, 2200, 540, 512];
const GB_INSTANCES: u32 = 32;
const FW_INSTANCES: u32 = 32;
const TILE_W: u32 = 64;
const TILE_H: u32 = 60;
const TILE_COLS: u32 = W / TILE_W;
const TILE_ROWS: u32 = H / TILE_H;
/// A UAV tile is a dispatch's (8x8)x(8,4) threads: 64x32 px.
const UAV_TILES_X: u32 = UAV_SIDE / 64;
const UAV_TILES_Y: u32 = UAV_SIDE / 32;

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}

/// The integer mix the shaders use (`hmix`), for the CPU side's seeds
/// and texels: every hash in the harness derives from [`SEED`].
fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// One texel of big texture `t` at `(x, y)`: a per-pixel hash crossed
/// with a coarser block tint, so the images carry structure and noise.
fn texel(t: usize, x: u32, y: u32) -> u32 {
    let n = mix(SEED
        ^ (t as u32)
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(x.wrapping_mul(0x85eb_ca6b))
            .wrapping_add(y.wrapping_mul(0xc2b2_ae35)));
    let b = mix(SEED
        ^ (t as u32)
            .wrapping_mul(7919)
            .wrapping_add((x / 64).wrapping_mul(131))
            .wrapping_add((y / 64).wrapping_mul(137)));
    let r = (n ^ b) as u8;
    let g = ((n >> 8) as u8) ^ (((x ^ y) / 8) as u8);
    let bl = ((b >> 16) as u8).wrapping_add(((x + y) / 4) as u8);
    u32::from_le_bytes([r, g, bl, 255])
}

/// FNV-1a over the readback's raw texel bytes.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// `dxc.exe` next to the SDK `dxil.dll` [`Validator::default_path`]
/// finds, or `MESIA_PC_DXC` (the same convention as `synth_scene`'s).
fn find_dxc() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MESIA_PC_DXC") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    Validator::default_path()?
        .with_file_name("dxc.exe")
        .is_file()
        .then(|| Validator::default_path().unwrap().with_file_name("dxc.exe"))
}

fn compile(dxc: &Path, dir: &Path, entry: &str, profile: &str) -> Vec<u8> {
    std::fs::create_dir_all(dir).unwrap();
    let src = dir.join("heavy.hlsl");
    if !src.exists() {
        std::fs::write(&src, HLSL).unwrap();
    }
    let out = dir.join(format!("{entry}.bin"));
    let r = std::process::Command::new(dxc)
        .args(["-T", profile, "-E", entry, "-Fo"])
        .arg(&out)
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "dxc {entry}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::read(&out).unwrap()
}

/// A dxc-compiled container as the host pipeline helpers take it; every
/// shader here declares the same sixteen-dword user cbuffer, described
/// as sixteen [`RootType::Constants`] dwords of root parameter 0.
fn shader(stage: HwStage, dxil: Vec<u8>) -> HostShader {
    HostShader {
        stage,
        dxil,
        user_data: (0..UD_DWORDS)
            .map(|i| UserData {
                index: i,
                slot: 0,
                root: RootType::Constants,
                dword: i,
            })
            .collect(),
        samplers: Vec::new(),
        static_samplers: Vec::new(),
        root_cbvs: Vec::new(),
        root_srvs: Vec::new(),
        root_uavs: Vec::new(),
        root_indexed_views: Vec::new(),
    }
}

fn buffer(size: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    }
}

fn texture2d(
    w: u32,
    h: u32,
    format: DXGI_FORMAT,
    flags: D3D12_RESOURCE_FLAGS,
) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(w),
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: flags,
        ..Default::default()
    }
}

fn transition(
    r: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: a borrowed pointer; the barrier never releases it.
                pResource: unsafe { std::mem::transmute_copy(r) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// The debug layer's stored messages (`MESIA_D3D12_DEBUG=1`), for a failure.
fn debug_messages(device: &ID3D12Device) -> String {
    let Ok(queue) = device.cast::<ID3D12InfoQueue>() else {
        return String::new();
    };
    let mut text = String::new();
    // SAFETY: each message is read into a buffer of the size asked for.
    unsafe {
        for i in 0..queue.GetNumStoredMessages() {
            let mut len = 0usize;
            let _ = queue.GetMessage(i, None, &mut len);
            let mut buffer = vec![0u64; len.div_ceil(8)];
            let m = buffer.as_mut_ptr().cast::<D3D12_MESSAGE>();
            if queue.GetMessage(i, Some(m), &mut len).is_ok() {
                let bytes = std::slice::from_raw_parts(
                    (*m).pDescription,
                    (*m).DescriptionByteLength.saturating_sub(1),
                );
                text.push_str("\n  ");
                text.push_str(&String::from_utf8_lossy(bytes));
            }
        }
    }
    text
}

/// `PROCESS_MEMORY_COUNTERS` (`psapi.h`), reached through kernel32's
/// `K32*` forwards so the test needs no extra `windows` feature.
#[repr(C)]
struct MemoryCounters {
    cb: u32,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_nonpaged: usize,
    quota_nonpaged: usize,
    pagefile: usize,
    peak_pagefile: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetCurrentProcess"]
    fn current_process() -> *mut std::ffi::c_void;
    #[link_name = "K32GetProcessMemoryInfo"]
    fn process_memory_info(h: *mut std::ffi::c_void, c: *mut MemoryCounters, cb: u32) -> i32;
}

/// The process's working set in bytes, or None if psapi is absent.
fn working_set() -> Option<usize> {
    // SAFETY: `c` is a valid PROCESS_MEMORY_COUNTERS of `cb` bytes.
    let mut c: MemoryCounters = unsafe { std::mem::zeroed() };
    c.cb = size_of::<MemoryCounters>() as u32;
    (unsafe { process_memory_info(current_process(), &mut c, c.cb) } != 0).then_some(c.working_set)
}

/// The adapter's name and local-segment VRAM budget/usage, where the
/// driver reports them (IDXGIAdapter3; WARP may decline the query).
fn adapter_info(dev: &ID3D12Device) -> Option<(String, u64, u64)> {
    // SAFETY: plain DXGI calls with valid out pointers.
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).ok()?;
        let adapter = factory
            .EnumAdapterByLuid::<IDXGIAdapter1>(dev.GetAdapterLuid())
            .ok()?;
        let desc = adapter.GetDesc1().unwrap_or_default();
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end]);
        let a3: IDXGIAdapter3 = adapter.cast().ok()?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        a3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
            .ok()?;
        Some((name, info.Budget, info.CurrentUsage))
    }
}

/// A graphics pipeline on the shared root signature: one POSITION
/// float2 input, `targets` RGBA8 targets, optionally a D32 depth test.
fn graphics_pso(
    dev: &ID3D12Device,
    root: &ID3D12RootSignature,
    vs: &[u8],
    ps: &[u8],
    targets: u32,
    depth: bool,
) -> Result<ID3D12PipelineState, String> {
    let elements = [D3D12_INPUT_ELEMENT_DESC {
        SemanticName: PCSTR(c"POSITION".as_ptr().cast()),
        SemanticIndex: 0,
        Format: DXGI_FORMAT_R32G32_FLOAT,
        InputSlot: 0,
        AlignedByteOffset: 0,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        InstanceDataStepRate: 0,
    }];
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        // SAFETY: a borrowed pointer the descriptor never releases.
        pRootSignature: unsafe { std::mem::transmute_copy(root) },
        VS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs.as_ptr().cast(),
            BytecodeLength: vs.len(),
        },
        PS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: ps.as_ptr().cast(),
            BytecodeLength: ps.len(),
        },
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: elements.as_ptr(),
            NumElements: elements.len() as u32,
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: targets,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    for f in desc.RTVFormats.iter_mut().take(targets as usize) {
        *f = FMT8;
    }
    for rt in &mut desc.BlendState.RenderTarget {
        rt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
    }
    if depth {
        desc.DSVFormat = DEPTH_FMT;
        desc.DepthStencilState = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(),
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS_EQUAL,
            ..Default::default()
        };
    }
    // SAFETY: the description points at live data.
    unsafe { dev.CreateGraphicsPipelineState(&desc) }.map_err(err)
}

struct Pipes {
    /// Shared root signature: VS constants at 0, PS at 1 (all stages
    /// declare the same sixteen-dword cbuffer).
    root: ID3D12RootSignature,
    gb: ID3D12PipelineState,
    fw: ID3D12PipelineState,
    tile: ID3D12PipelineState,
    cs_root: ID3D12RootSignature,
    cs: ID3D12PipelineState,
}

struct Slot {
    alloc: ID3D12CommandAllocator,
    /// The submitted list, kept until the GPU passes its fence.
    list: Option<ID3D12GraphicsCommandList>,
}

/// The device side: the four render targets and their views, the depth
/// buffer, the four big textures, the compute UAV, the shared quad
/// vertex and index buffers, the timestamp heap and its resolve buffer,
/// the present readback, and the submission ring.
struct Load {
    host: Host,
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    signaled: u64,
    queries: ID3D12QueryHeap,
    resolve: ID3D12Resource,
    readback: ID3D12Resource,
    rt: [ID3D12Resource; 4],
    tex: [ID3D12Resource; N_TEX],
    vb: ID3D12Resource,
    ib: ID3D12Resource,
    /// Resources the GPU uses without a further borrow: the depth buffer
    /// and the compute UAV.
    keep: Vec<ID3D12Resource>,
    vbv: D3D12_VERTEX_BUFFER_VIEW,
    ibv: D3D12_INDEX_BUFFER_VIEW,
    /// The descriptor heaps the handles below index into; they must
    /// outlive every recorded frame, so they live here.
    heaps: Vec<ID3D12DescriptorHeap>,
    views: ID3D12DescriptorHeap,
    rtv: [D3D12_CPU_DESCRIPTOR_HANDLE; 4],
    dsv: D3D12_CPU_DESCRIPTOR_HANDLE,
    slots: Vec<Slot>,
}

impl Load {
    /// SAFETY: plain D3D12 calls whose descriptions and uploads outlive
    /// them; the init list's fence is waited on before the function
    /// returns, after which every resource lives in `self`.
    unsafe fn new(host: Host, frames: usize) -> Result<Self, String> {
        let dev = host.device.clone();
        let queue: ID3D12CommandQueue = unsafe {
            dev.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let heap = |t| D3D12_HEAP_PROPERTIES {
            Type: t,
            ..Default::default()
        };
        let make = |ty: D3D12_HEAP_TYPE,
                    desc: &D3D12_RESOURCE_DESC,
                    state: D3D12_RESOURCE_STATES,
                    clear: Option<&D3D12_CLEAR_VALUE>| {
            let mut r: Option<ID3D12Resource> = None;
            unsafe {
                dev.CreateCommittedResource(
                    &heap(ty),
                    D3D12_HEAP_FLAG_NONE,
                    desc,
                    state,
                    clear.map(|c| c as *const _),
                    &mut r,
                )
            }
            .map_err(err)?;
            r.ok_or_else(|| "no resource".to_string())
        };
        let rt_clear = |c: [f32; 4]| D3D12_CLEAR_VALUE {
            Format: FMT8,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: c },
        };
        let depth_clear = D3D12_CLEAR_VALUE {
            Format: DEPTH_FMT,
            Anonymous: D3D12_CLEAR_VALUE_0 {
                DepthStencil: D3D12_DEPTH_STENCIL_VALUE {
                    Depth: 1.0,
                    Stencil: 0,
                },
            },
        };
        // The four targets: rt[0..2] are the gbuffer MRT, rt[2] the
        // forward target, rt[3] the composite's output that is copied
        // out as the present stand-in.
        let mut rt = Vec::with_capacity(4);
        for _ in 0..4 {
            rt.push(make(
                D3D12_HEAP_TYPE_DEFAULT,
                &texture2d(W, H, FMT8, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                Some(&rt_clear([0.0; 4])),
            )?);
        }
        let rt: [ID3D12Resource; 4] = rt.try_into().map_err(|_| "rt count".to_string())?;
        let depth = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &texture2d(W, H, DEPTH_FMT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL),
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(&depth_clear),
        )?;
        let uav = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &texture2d(
                UAV_SIDE,
                UAV_SIDE,
                FMT8,
                D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ),
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            None,
        )?;
        let mut tex = Vec::with_capacity(N_TEX);
        for _ in 0..N_TEX {
            tex.push(make(
                D3D12_HEAP_TYPE_DEFAULT,
                &texture2d(TEX, TEX, FMT8, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
            )?);
        }
        let tex: [ID3D12Resource; N_TEX] = tex.try_into().map_err(|_| "tex count".to_string())?;
        // The four texture uploads, generated straight into the upload
        // heaps; each row is TEX*4 bytes, already 256-aligned.
        let pitch = (TEX * 4) as usize;
        let mut uploads = Vec::with_capacity(N_TEX);
        for t in 0..N_TEX {
            let up = make(
                D3D12_HEAP_TYPE_UPLOAD,
                &buffer((pitch * TEX as usize) as u64),
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
            )?;
            unsafe {
                let mut p = std::ptr::null_mut();
                up.Map(0, None, Some(&mut p)).map_err(err)?;
                for y in 0..TEX {
                    let row = (p as *mut u8).add(y as usize * pitch).cast::<u32>();
                    for x in 0..TEX {
                        row.add(x as usize).write_unaligned(texel(t, x, y));
                    }
                }
                up.Unmap(0, None);
            }
            uploads.push(up);
        }
        // The quad vertex buffer and the shared fan indices, uploaded to
        // default-heap buffers through one staging buffer.
        let vb = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &buffer(32),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        let ib = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &buffer(12),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        let geo = make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer(64),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let quad: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        unsafe {
            let mut p = std::ptr::null_mut();
            geo.Map(0, None, Some(&mut p)).map_err(err)?;
            std::ptr::copy_nonoverlapping(quad.as_ptr(), (p as *mut u8).cast::<f32>(), 8);
            std::ptr::copy_nonoverlapping(
                indices.as_ptr(),
                (p as *mut u8).add(32).cast::<u16>(),
                6,
            );
            geo.Unmap(0, None);
        }
        let rtvs: ID3D12DescriptorHeap = unsafe {
            dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 4,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let dsvs: ID3D12DescriptorHeap = unsafe {
            dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
                NumDescriptors: 1,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let views: ID3D12DescriptorHeap = unsafe {
            dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 8,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let rtv_size =
            unsafe { dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) }
                as usize;
        let view_size =
            unsafe { dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) }
                as usize;
        let rtv0 = unsafe { rtvs.GetCPUDescriptorHandleForHeapStart() };
        let mut rtv = [rtv0; 4];
        for (i, h) in rtv.iter_mut().enumerate() {
            h.ptr += rtv_size * i;
            unsafe { dev.CreateRenderTargetView(&rt[i], None, *h) };
        }
        let dsv = unsafe { dsvs.GetCPUDescriptorHandleForHeapStart() };
        let dsv_desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DEPTH_FMT,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };
        unsafe { dev.CreateDepthStencilView(&depth, Some(&dsv_desc), dsv) };
        // The shader-visible heap: 0..4 the big textures, 4..7 the first
        // three targets' SRVs, 7 the compute UAV.
        let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: FMT8,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        let mut at = unsafe { views.GetCPUDescriptorHandleForHeapStart() };
        for t in &tex {
            unsafe { dev.CreateShaderResourceView(t, Some(&srv_desc), at) };
            at.ptr += view_size;
        }
        for t in &rt[..3] {
            unsafe { dev.CreateShaderResourceView(t, Some(&srv_desc), at) };
            at.ptr += view_size;
        }
        let uav_desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: FMT8,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_UAV {
                    MipSlice: 0,
                    PlaneSlice: 0,
                },
            },
        };
        unsafe { dev.CreateUnorderedAccessView(&uav, None, Some(&uav_desc), at) };
        let readback = make(
            D3D12_HEAP_TYPE_READBACK,
            &buffer((W * H * 4) as u64),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        let resolve = make(
            D3D12_HEAP_TYPE_READBACK,
            &buffer((frames * 3 * 8) as u64),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        let mut queries: Option<ID3D12QueryHeap> = None;
        unsafe {
            dev.CreateQueryHeap(
                &D3D12_QUERY_HEAP_DESC {
                    Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
                    Count: (frames * 3) as u32,
                    NodeMask: 0,
                },
                &mut queries,
            )
        }
        .map_err(err)?;
        let queries = queries.ok_or_else(|| "no query heap".to_string())?;
        let fence: ID3D12Fence =
            unsafe { dev.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.map_err(err)?;
        let mut slots = Vec::with_capacity(IN_FLIGHT);
        for _ in 0..IN_FLIGHT {
            slots.push(Slot {
                alloc: unsafe { dev.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }
                    .map_err(err)?,
                list: None,
            });
        }
        let mut load = Load {
            host,
            queue,
            fence,
            signaled: 0,
            queries,
            resolve,
            readback,
            rt,
            tex,
            keep: vec![depth, uav],
            vbv: D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: unsafe { vb.GetGPUVirtualAddress() },
                SizeInBytes: 32,
                StrideInBytes: 8,
            },
            ibv: D3D12_INDEX_BUFFER_VIEW {
                BufferLocation: unsafe { ib.GetGPUVirtualAddress() },
                SizeInBytes: 12,
                Format: DXGI_FORMAT_R16_UINT,
            },
            vb,
            ib,
            heaps: vec![rtvs, dsvs],
            views,
            rtv,
            dsv,
            slots,
        };
        // The init submission: the texture and geometry copies, then the
        // transitions into their working states. Its fence is waited on
        // here, so the upload heaps die with the function's scope.
        unsafe {
            let alloc: ID3D12CommandAllocator = dev
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(err)?;
            let list: ID3D12GraphicsCommandList = dev
                .CreateCommandList(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    &alloc,
                    Option::<&ID3D12PipelineState>::None,
                )
                .map_err(err)?;
            let subresource = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(r),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let footprint = |r: &ID3D12Resource, w: u32, h: u32| D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(r),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: FMT8,
                            Width: w,
                            Height: h,
                            Depth: 1,
                            RowPitch: w * 4,
                        },
                    },
                },
            };
            for (t, up) in load.tex.iter().zip(&uploads) {
                list.CopyTextureRegion(&subresource(t), 0, 0, 0, &footprint(up, TEX, TEX), None);
            }
            list.CopyBufferRegion(&load.vb, 0, &geo, 0, 32);
            list.CopyBufferRegion(&load.ib, 0, &geo, 32, 12);
            let mut barriers: Vec<D3D12_RESOURCE_BARRIER> = load
                .tex
                .iter()
                .map(|t| {
                    transition(
                        t,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    )
                })
                .collect();
            barriers.push(transition(
                &load.vb,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER,
            ));
            barriers.push(transition(
                &load.ib,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_INDEX_BUFFER,
            ));
            list.ResourceBarrier(&barriers);
            list.Close().map_err(err)?;
            load.queue
                .ExecuteCommandLists(&[Some(list.cast().map_err(err)?)]);
            load.queue.Signal(&load.fence, 1).map_err(err)?;
            load.signaled = 1;
            load.wait(1)?;
        }
        drop(uploads);
        drop(geo);
        Ok(load)
    }

    /// Spins until the fence reaches `v`, reporting a device removal.
    fn wait(&self, v: u64) -> Result<(), String> {
        while unsafe { self.fence.GetCompletedValue() } < v {
            if let Err(e) = unsafe { self.host.device.GetDeviceRemovedReason() } {
                return Err(format!("device removed: {}", err(e)));
            }
            std::thread::yield_now();
        }
        Ok(())
    }

    /// Waits for every submission so far.
    fn drain(&self) -> Result<(), String> {
        self.wait(self.signaled)
    }

    /// One frame's command list: the four passes between TIMESTAMP
    /// queries, then the present copy and the resolve. Returns
    /// `(queue-wait ms, submit ms)`; the GPU times are read back at the
    /// end. SAFETY: descriptions and uploads outlive the recording; the
    /// slot's fence wait keeps every resource alive until the GPU has
    /// finished with the frame that occupied it.
    fn frame(&mut self, p: &Pipes, counts: &[u32; 4], f: usize) -> Result<(f64, f64), String> {
        let dev = &self.host.device;
        let slot = f % IN_FLIGHT;
        let wait_start = Instant::now();
        // The slot's previous frame must be done on the GPU before its
        // allocator records again.
        let need = (self.signaled + 1).saturating_sub(IN_FLIGHT as u64);
        if need > 0 {
            self.wait(need)?;
        }
        let waited = wait_start.elapsed().as_secs_f64() * 1e3;
        let mut vc = [0u32; UD_DWORDS as usize];
        let mut pc = [0u32; UD_DWORDS as usize];
        let mut cc = [0u32; UD_DWORDS as usize];
        // A draw's constant buffer: sixteen dwords of (draw, pass) hash,
        // the per-draw data a title pushes.
        let fill = |buf: &mut [u32; 16], a: u32, b: u32| {
            for (i, w) in buf.iter_mut().enumerate() {
                *w = mix(a ^ b ^ (i as u32).wrapping_mul(0x27d4_eb2f));
            }
        };
        unsafe {
            let base = (f * 3) as u32;
            let cpu = Instant::now();
            let slot = &mut self.slots[slot];
            slot.alloc.Reset().map_err(err)?;
            let list: ID3D12GraphicsCommandList = dev
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &slot.alloc, &p.gb)
                .map_err(err)?;
            list.EndQuery(&self.queries, D3D12_QUERY_TYPE_TIMESTAMP, base);
            list.SetDescriptorHeaps(&[Some(self.views.clone())]);
            let vp = [D3D12_VIEWPORT {
                Width: W as f32,
                Height: H as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }];
            let sc = [RECT {
                right: W as i32,
                bottom: H as i32,
                ..Default::default()
            }];
            // Pass 1: the gbuffer draws, MRT x2 with depth.
            list.SetGraphicsRootSignature(&p.root);
            list.SetPipelineState(&p.gb);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.IASetVertexBuffers(0, Some(&[self.vbv]));
            list.IASetIndexBuffer(Some(&self.ibv));
            list.OMSetRenderTargets(2, Some(self.rtv.as_ptr()), false, Some(&self.dsv));
            list.RSSetViewports(&vp);
            list.RSSetScissorRects(&sc);
            list.ClearRenderTargetView(self.rtv[0], &[0.03125, 0.0625, 0.125, 1.0], None);
            list.ClearRenderTargetView(self.rtv[1], &[0.0; 4], None);
            list.ClearDepthStencilView(self.dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);
            for d in 0..counts[0] {
                let s = mix(SEED ^ d.wrapping_mul(0x9e37_79b9));
                fill(&mut vc, s, d);
                fill(&mut pc, s, d ^ 0xffff);
                list.SetGraphicsRoot32BitConstants(0, UD_DWORDS, vc.as_ptr().cast(), 0);
                list.SetGraphicsRoot32BitConstants(1, UD_DWORDS, pc.as_ptr().cast(), 0);
                list.DrawIndexedInstanced(6, GB_INSTANCES, 0, 0, 0);
            }
            // Pass 2: the forward draws, a third target, depth again.
            list.SetPipelineState(&p.fw);
            list.OMSetRenderTargets(1, Some(&self.rtv[2]), false, Some(&self.dsv));
            list.ClearRenderTargetView(self.rtv[2], &[0.0; 4], None);
            list.ClearDepthStencilView(self.dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);
            for d in 0..counts[1] {
                let s = mix(SEED ^ 0x85eb_ca6b_u32.wrapping_mul(d + 1));
                fill(&mut vc, s, d ^ 0x1234);
                fill(&mut pc, s, d ^ 0x7fff);
                list.SetGraphicsRoot32BitConstants(0, UD_DWORDS, vc.as_ptr().cast(), 0);
                list.SetGraphicsRoot32BitConstants(1, UD_DWORDS, pc.as_ptr().cast(), 0);
                list.DrawIndexedInstanced(6, FW_INSTANCES, 0, 0, 0);
            }
            // The composite reads the three targets back.
            list.ResourceBarrier(&[
                transition(
                    &self.rt[0],
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
                transition(
                    &self.rt[1],
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
                transition(
                    &self.rt[2],
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
            ]);
            // Pass 3: the tiled composite into the present target.
            list.SetPipelineState(&p.tile);
            list.OMSetRenderTargets(1, Some(&self.rtv[3]), false, None);
            list.ClearRenderTargetView(self.rtv[3], &[0.0; 4], None);
            for d in 0..counts[2] {
                vc[0] = (d % TILE_COLS) * TILE_W;
                vc[1] = (d / TILE_COLS % TILE_ROWS) * TILE_H;
                vc[2] = mix(SEED ^ d);
                list.SetGraphicsRoot32BitConstants(0, UD_DWORDS, vc.as_ptr().cast(), 0);
                list.DrawIndexedInstanced(6, 1, 0, 0, 0);
            }
            list.ResourceBarrier(&[
                transition(
                    &self.rt[0],
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                ),
                transition(
                    &self.rt[1],
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                ),
                transition(
                    &self.rt[2],
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                ),
                transition(
                    &self.rt[3],
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
            ]);
            // Pass 4: the compute tail.
            list.SetComputeRootSignature(&p.cs_root);
            list.SetPipelineState(&p.cs);
            for d in 0..counts[3] {
                cc[0] = (d % UAV_TILES_X) * 64;
                cc[1] = (d / UAV_TILES_X % UAV_TILES_Y) * 32;
                cc[2] = mix(SEED ^ d.wrapping_mul(0x27d4_eb2f));
                list.SetComputeRoot32BitConstants(0, UD_DWORDS, cc.as_ptr().cast(), 0);
                list.Dispatch(8, 4, 1);
            }
            list.EndQuery(&self.queries, D3D12_QUERY_TYPE_TIMESTAMP, base + 1);
            // The present stand-in: the final target to the readback.
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&self.readback),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: FMT8,
                            Width: W,
                            Height: H,
                            Depth: 1,
                            RowPitch: W * 4,
                        },
                    },
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&self.rt[3]),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            list.ResourceBarrier(&[transition(
                &self.rt[3],
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )]);
            list.EndQuery(&self.queries, D3D12_QUERY_TYPE_TIMESTAMP, base + 2);
            list.ResolveQueryData(
                &self.queries,
                D3D12_QUERY_TYPE_TIMESTAMP,
                base,
                3,
                &self.resolve,
                u64::from(base) * 8,
            );
            list.Close().map_err(err)?;
            self.queue
                .ExecuteCommandLists(&[Some(list.cast().map_err(err)?)]);
            self.signaled += 1;
            self.queue.Signal(&self.fence, self.signaled).map_err(err)?;
            let submit = cpu.elapsed().as_secs_f64() * 1e3;
            slot.list = Some(list);
            let _ = (&self.keep, &self.heaps);
            Ok((waited, submit))
        }
    }
}

/// One measured frame's times, in milliseconds.
struct Row {
    cpu: f64,
    qwait: f64,
    render: f64,
    copy: f64,
    total: f64,
}

/// The `q` quantile of sorted `v` by the nearest-rank method.
fn pctile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[(((sorted.len() as f64) * q).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)]
}

fn stats(name: &str, v: &[f64]) {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let mean = s.iter().sum::<f64>() / s.len().max(1) as f64;
    println!(
        "  {name:<16} p50={:>8.2} p95={:>8.2} p99={:>8.2} mean={:>8.2} min={:>8.2} max={:>8.2}",
        pctile(&s, 0.50),
        pctile(&s, 0.95),
        pctile(&s, 0.99),
        mean,
        s.first().copied().unwrap_or(0.0),
        s.last().copied().unwrap_or(0.0),
    );
}

/// Compiles the shaders, builds the pipelines and the scene, submits
/// `frames`+`WARMUP` frames, then reads the timestamps and the present
/// target back and prints the table.
fn run(host: Host, dxc: &Path) -> Result<(), String> {
    let frames = std::env::var("MESIA_HEAVY_FRAMES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(120)
        .max(1);
    let mult = std::env::var("MESIA_HEAVY_DRAWS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .max(0.001);
    let counts = PASSES.map(|n| ((n as f64 * mult).round() as u32).max(1));
    let total = frames + WARMUP;
    let dir = std::env::temp_dir().join(format!("mesia-heavy-{}", std::process::id()));
    let compile = |entry: &str| {
        self::compile(
            dxc,
            &dir,
            entry,
            if entry.starts_with("cs") {
                "cs_6_6"
            } else if entry.starts_with("vs") {
                "vs_6_6"
            } else {
                "ps_6_6"
            },
        )
    };
    let vs_gb = shader(HwStage::Vs, compile("vs_gb"));
    let ps_gb = shader(HwStage::Ps, compile("ps_gb"));
    let vs_fw = shader(HwStage::Vs, compile("vs_fw"));
    let ps_fw = shader(HwStage::Ps, compile("ps_fw"));
    let vs_tile = shader(HwStage::Vs, compile("vs_tile"));
    let ps_tile = shader(HwStage::Ps, compile("ps_tile"));
    let cs = shader(HwStage::Cs, compile("cs_work"));
    let dev = &host.device;
    let root = host.root_signature(&vs_gb, &ps_gb)?;
    let (cs_root, cs_pso) = host.compute_pipeline(&cs)?;
    let pipes = Pipes {
        gb: graphics_pso(dev, &root, &vs_gb.dxil, &ps_gb.dxil, 2, true)?,
        fw: graphics_pso(dev, &root, &vs_fw.dxil, &ps_fw.dxil, 1, true)?,
        tile: graphics_pso(dev, &root, &vs_tile.dxil, &ps_tile.dxil, 1, false)?,
        root,
        cs_root,
        cs: cs_pso,
    };
    let adapter = adapter_info(dev);
    let mut load = unsafe { Load::new(host, total)? };
    let freq = unsafe { load.queue.GetTimestampFrequency() }.map_err(err)? as f64;
    println!("== MESIA heavy synthetic load ==");
    println!(
        "  adapter  : {}",
        adapter
            .as_ref()
            .map_or("unknown".to_string(), |a| a.0.clone())
    );
    println!("  dxc      : {}", dxc.display());
    println!(
        "  target   : {W}x{H} RGBA8 x4 + D32; uav {UAV_SIDE}x{UAV_SIDE}; tex {N_TEX}x {TEX}x{TEX} RGBA8"
    );
    println!(
        "  passes   : gbuffer {}x{} + forward {}x{} + composite {} + compute {} = {} submits/frame (x{mult:.3})",
        counts[0],
        GB_INSTANCES,
        counts[1],
        FW_INSTANCES,
        counts[2],
        counts[3],
        counts.iter().sum::<u32>(),
    );
    println!(
        "  frames   : {frames} measured + {WARMUP} warmup, {IN_FLIGHT} in flight; ts freq {freq:.0} Hz"
    );
    println!();
    println!(
        " {:>5} {:>10} {:>10} {:>12} {:>10} {:>8}",
        "frame", "cpu_ms", "qwait_ms", "gpu_total_ms", "render_ms", "copy_ms"
    );
    let wall = Instant::now();
    let mut rows = Vec::with_capacity(frames);
    let mut timing = Vec::with_capacity(frames);
    for f in 0..total {
        let (waited, submit) = load.frame(&pipes, &counts, f)?;
        if f >= WARMUP {
            timing.push((waited, submit));
        }
    }
    load.drain()?;
    let wall_s = wall.elapsed().as_secs_f64();
    // The timestamp triples, then the present target's readback.
    let mut ts = vec![0u64; total * 3];
    unsafe {
        let mut p = std::ptr::null_mut();
        load.resolve.Map(0, None, Some(&mut p)).map_err(err)?;
        std::ptr::copy_nonoverlapping(p as *const u64, ts.as_mut_ptr(), ts.len());
        load.resolve.Unmap(0, None);
    }
    for (i, &(waited, submit)) in timing.iter().enumerate() {
        let f = i + WARMUP;
        let (t0, t1, t2) = (ts[f * 3], ts[f * 3 + 1], ts[f * 3 + 2]);
        let (render, copy) = if t2 > t0 {
            (
                (t1.saturating_sub(t0)) as f64 / freq * 1e3,
                (t2.saturating_sub(t1)) as f64 / freq * 1e3,
            )
        } else {
            (0.0, 0.0)
        };
        rows.push(Row {
            cpu: submit,
            qwait: waited,
            render,
            copy,
            total: render + copy,
        });
    }
    for (i, r) in rows.iter().enumerate() {
        println!(
            " {:>5} {:>10.2} {:>10.2} {:>12.2} {:>10.2} {:>8.2}",
            i, r.cpu, r.qwait, r.total, r.render, r.copy
        );
    }
    println!();
    println!("  summary over {} frames (ms):", rows.len());
    stats(
        "cpu submit",
        &rows.iter().map(|r| r.cpu).collect::<Vec<_>>(),
    );
    stats(
        "queue wait",
        &rows.iter().map(|r| r.qwait).collect::<Vec<_>>(),
    );
    stats(
        "gpu render",
        &rows.iter().map(|r| r.render).collect::<Vec<_>>(),
    );
    stats("gpu copy", &rows.iter().map(|r| r.copy).collect::<Vec<_>>());
    stats(
        "gpu total",
        &rows.iter().map(|r| r.total).collect::<Vec<_>>(),
    );
    println!("  wall: {wall_s:.2}s for {total} submissions");
    if let Some(ws) = working_set() {
        println!("  ram : working set {:.0} MiB", ws as f64 / 1_048_576.0);
    }
    if let Some((_, budget, usage)) = adapter_info(&load.host.device) {
        println!(
            "  vram: usage {:.0} / budget {:.0} MiB (local segment)",
            usage as f64 / 1_048_576.0,
            budget as f64 / 1_048_576.0
        );
    }
    let mut img = vec![0u8; (W * H * 4) as usize];
    unsafe {
        let mut p = std::ptr::null_mut();
        load.readback.Map(0, None, Some(&mut p)).map_err(err)?;
        let pitch = (W * 4) as usize;
        for y in 0..H as usize {
            std::ptr::copy_nonoverlapping(
                (p as *const u8).add(y * pitch),
                img.as_mut_ptr().add(y * pitch),
                pitch,
            );
        }
        load.readback.Unmap(0, None);
    }
    let h = fnv1a(&img);
    if !img.iter().any(|&b| b != 0) {
        eprintln!("  warning: the last frame read back all-zero");
    }
    println!("  hash: FNV-1a(rt3, {} B) = {h:#018x}", img.len());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The reference load run. `MESIA_HEAVY=1` opts in - without it this is
/// a fast skip so `cargo test` stays a CI pass.
#[test]
fn heavy_load_reference() {
    if std::env::var_os("MESIA_HEAVY").is_none_or(|v| v != "1") {
        eprintln!("skipped, set MESIA_HEAVY=1");
        return;
    }
    let Some(dxc) = find_dxc() else {
        eprintln!("no PC dxc; skipped");
        return;
    };
    // `MESIA_WARP=1` picks the software adapter, `Host::new`'s
    // convention; the load runs there too, only slower - scale it down
    // with MESIA_HEAVY_DRAWS (e.g. 0.02) and MESIA_HEAVY_FRAMES.
    let warp = std::env::var_os("MESIA_WARP").is_some_and(|v| v == "1");
    let host = match Host::new(warp) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("no usable device ({e}); skipped");
            return;
        }
    };
    let dev = host.device.clone();
    if let Err(e) = run(host, &dxc) {
        panic!("heavy load failed: {e}{}", debug_messages(&dev));
    }
}

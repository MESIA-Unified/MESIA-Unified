//! Compute correctness, wave two: the subtler device behaviours the
//! translator and lowering rely on. Same convention as
//! `compute_correctness.rs` - original HLSL compiled at test time with the
//! SDK's `dxc.exe`, bound through [`mesia_shader::emit::host::Host`]'s
//! pipeline helpers (constants at `b0, space0`; root `t`/`u` views in
//! space 1; SM 6.6 heap indexing), dispatched on hardware - or on WARP
//! with `MESIA_WARP=1`.
//!
//! Covered here (deliberately *not* what wave one already checks):
//!
//! * bank-conflict-style groupshared traffic: stride-32 permutations and
//!   padded rows whose values must survive whatever the banks cost;
//! * UAV atomics under a thousand-way contention, exact final values for
//!   add/min/max/or/and/xor plus exchange and compare-exchange;
//! * cross-lane exchange *without* wave intrinsics - lanes publish to
//!   groupshared, sync, and read a permutation, the pattern wave64->32
//!   lowering emulates a wave64 shuffle with;
//! * the `v_mbcnt`-style pattern: a per-lane predicate packed into 32-bit
//!   mask words, a `countbits` exclusive prefix, and a compaction scatter
//!   driven by it;
//! * read-after-write hazards inside one dispatch across
//!   `AllMemoryBarrierWithGroupSync` phases - stale reads must not leak;
//! * dispatch-to-dispatch bitwise determinism;
//! * half-precision round-trips and unorm/pack edge conversions against
//!   an exact CPU model (one documented byte of slack on
//!   `D3DCOLORtoUBYTE4`, whose rounding the spec leaves room on).
//!
//! Every check is exact integers (or exact f32 bits) unless documented
//! otherwise; mismatches print `label[index]: got != want` and fail.
//! Skipped without a compiler or an SM 6.6 device, like wave one.

#![cfg(all(windows, feature = "host"))]

use std::path::{Path, PathBuf};

use mesia_shader::HwStage;
use mesia_shader::emit::HostShader;
use mesia_shader::emit::host::Host;
use mesia_shader::emit::validator::Validator;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}

/// `dxc.exe` next to the SDK `dxil.dll` [`Validator::default_path`] finds,
/// or `MESIA_PC_DXC` (the same convention as `synth_scene`).
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

/// Compiles `src` (written to `dir/name.hlsl`) for `entry` at `profile`.
fn compile(dxc: &Path, dir: &Path, name: &str, src: &str, entry: &str, profile: &str) -> Vec<u8> {
    std::fs::create_dir_all(dir).unwrap();
    let file = dir.join(format!("{name}.hlsl"));
    std::fs::write(&file, src).unwrap();
    let out = dir.join(format!("{name}-{entry}.bin"));
    let r = std::process::Command::new(dxc)
        .args(["-T", profile, "-E", entry, "-Fo"])
        .arg(&out)
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "dxc {name}/{entry}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::read(&out).unwrap()
}

/// A dxc-compiled container plus the root views `Host`'s root signature
/// must declare; none of the tests' shaders take user data or samplers.
fn shader(stage: HwStage, dxil: Vec<u8>, root_srvs: Vec<u8>, root_uavs: Vec<u8>) -> HostShader {
    HostShader {
        stage,
        dxil,
        user_data: Vec::new(),
        samplers: Vec::new(),
        static_samplers: Vec::new(),
        root_cbvs: Vec::new(),
        root_srvs,
        root_uavs,
        root_indexed_views: Vec::new(),
    }
}

/// The parameter index `Host::root_signature_of` gives a root view: one
/// constants parameter per stage, then views sorted by (slot, kind) -
/// kind 0 CBV, 1 SRV, 2 UAV - deduplicated across stages.
fn view_param(stages: &[&HostShader], kind: u8, slot: u8) -> u32 {
    let mut views: Vec<(u8, u8)> = stages
        .iter()
        .flat_map(|s| {
            let cbvs = s.root_cbvs.iter().map(|&r| (r, 0u8));
            let srvs = s.root_srvs.iter().map(|&r| (r, 1u8));
            cbvs.chain(srvs)
                .chain(s.root_uavs.iter().map(|&r| (r, 2u8)))
        })
        .collect();
    views.sort_unstable();
    views.dedup();
    let at = views
        .iter()
        .position(|&v| v == (slot, kind))
        .expect("the stage declares the view");
    (stages.len() + at) as u32
}

fn buffer(size: u64, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
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

/// Writes a slice into the start of an upload-heap resource.
/// SAFETY: `r` is an upload resource of at least `data`'s size.
unsafe fn write_upload<T>(r: &ID3D12Resource, data: &[T]) -> Result<(), String> {
    let mut p = std::ptr::null_mut();
    unsafe { r.Map(0, None, Some(&mut p)) }.map_err(err)?;
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), p.cast::<T>(), data.len());
        r.Unmap(0, None);
    }
    Ok(())
}

/// Reads `n` dwords from the start of a readback-heap resource.
/// SAFETY: `r` is a readback resource the GPU is done with.
unsafe fn read_dwords(r: &ID3D12Resource, n: usize) -> Result<Vec<u32>, String> {
    let mut p = std::ptr::null_mut();
    unsafe { r.Map(0, None, Some(&mut p)) }.map_err(err)?;
    let out = unsafe { std::slice::from_raw_parts(p.cast::<u32>(), n) }.to_vec();
    unsafe { r.Unmap(0, None) };
    Ok(out)
}

/// The device-side bits the tests share: a queue, one-shot command
/// recording under a fence, and committed-resource creation.
struct Gpu {
    host: Host,
    queue: ID3D12CommandQueue,
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let host = Host::new(false)?; // `MESIA_WARP=1` makes this the WARP adapter.
        // SAFETY: a plain D3D12 call with a valid out pointer.
        let queue: ID3D12CommandQueue = unsafe {
            host.device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })
        }
        .map_err(err)?;
        Ok(Gpu { host, queue })
    }

    fn make(
        &self,
        ty: D3D12_HEAP_TYPE,
        desc: &D3D12_RESOURCE_DESC,
        state: D3D12_RESOURCE_STATES,
        clear: Option<&D3D12_CLEAR_VALUE>,
    ) -> Result<ID3D12Resource, String> {
        let mut r: Option<ID3D12Resource> = None;
        // SAFETY: descriptions outlive the call; `r` is a valid out pointer.
        unsafe {
            self.host.device.CreateCommittedResource(
                &D3D12_HEAP_PROPERTIES {
                    Type: ty,
                    ..Default::default()
                },
                D3D12_HEAP_FLAG_NONE,
                desc,
                state,
                clear.map(|c| c as *const _),
                &mut r,
            )
        }
        .map_err(err)?;
        r.ok_or_else(|| "no resource".to_string())
    }

    /// Records `f` on a fresh direct list (initialised with `pso`), submits
    /// it and waits out a fence. Everything `f` used must be alive on the
    /// caller's stack across this call - the wait makes that enough.
    /// SAFETY: `f`'s D3D12 calls are valid and the resources they use
    /// outlive the submission.
    unsafe fn run(
        &self,
        pso: &ID3D12PipelineState,
        f: impl FnOnce(&ID3D12GraphicsCommandList),
    ) -> Result<(), String> {
        let dev = &self.host.device;
        // SAFETY: descriptions outlive the recording; the fence wait keeps
        // every resource alive until the GPU has finished the list.
        unsafe {
            let alloc: ID3D12CommandAllocator = dev
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(err)?;
            let list: ID3D12GraphicsCommandList = dev
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, pso)
                .map_err(err)?;
            f(&list);
            list.Close().map_err(err)?;
            self.queue
                .ExecuteCommandLists(&[Some(list.cast().map_err(err)?)]);
            let fence: ID3D12Fence = dev.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(err)?;
            self.queue.Signal(&fence, 1).map_err(err)?;
            while fence.GetCompletedValue() < 1 {
                if let Err(e) = dev.GetDeviceRemovedReason() {
                    return Err(format!("device removed: {}", err(e)));
                }
                std::thread::yield_now();
            }
        }
        Ok(())
    }
}

/// Builds `cs`'s compute pipeline, runs it once over a `dwords`-word raw
/// UAV (`u0, space1`, allocated here) and returns its contents. `bind` may
/// attach heaps or further root arguments before the dispatch.
/// SAFETY: as [`Gpu::run`].
unsafe fn dispatch_read(
    gpu: &Gpu,
    cs: &HostShader,
    dwords: u32,
    groups: [u32; 3],
    bind: impl FnOnce(&ID3D12GraphicsCommandList),
) -> Result<Vec<u32>, String> {
    let (root, pso) = gpu.host.compute_pipeline(cs)?;
    let out = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &buffer(
            u64::from(dwords) * 4,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ),
        D3D12_RESOURCE_STATE_COMMON,
        None,
    )?;
    let rb = gpu.make(
        D3D12_HEAP_TYPE_READBACK,
        &buffer(u64::from(dwords) * 4, D3D12_RESOURCE_FLAG_NONE),
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
    )?;
    let uav_at = view_param(&[cs], 2, 0);
    let va = unsafe { out.GetGPUVirtualAddress() };
    unsafe {
        gpu.run(&pso, |list| {
            list.SetComputeRootSignature(&root);
            bind(list);
            list.SetComputeRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            list.SetComputeRootUnorderedAccessView(uav_at, va);
            list.Dispatch(groups[0], groups[1], groups[2]);
            // (Buffers were promoted from the common state by the dispatch.)
            list.ResourceBarrier(&[transition(
                &out,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.CopyBufferRegion(&rb, 0, &out, 0, u64::from(dwords) * 4);
        })
    }?;
    unsafe { read_dwords(&rb, dwords as usize) }
}

/// A dispatch with data staged into its views: `srv_data` fills a
/// `t0, space1` `ByteAddressBuffer` (bound as a root SRV) and `uav_init`
/// preloads the `u0, space1` output UAV, both via upload copies in the
/// same command list (which also covers copy->shader visibility). Returns
/// the output UAV's `uav_dwords` words.
/// SAFETY: as [`Gpu::run`]; the shader must declare the views it binds.
unsafe fn dispatch_full(
    gpu: &Gpu,
    cs: &HostShader,
    srv_data: Option<&[u32]>,
    uav_init: Option<&[u32]>,
    uav_dwords: u32,
    groups: [u32; 3],
) -> Result<Vec<u32>, String> {
    let (root, pso) = gpu.host.compute_pipeline(cs)?;
    let out = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &buffer(
            u64::from(uav_dwords) * 4,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ),
        D3D12_RESOURCE_STATE_COMMON,
        None,
    )?;
    let rb = gpu.make(
        D3D12_HEAP_TYPE_READBACK,
        &buffer(u64::from(uav_dwords) * 4, D3D12_RESOURCE_FLAG_NONE),
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
    )?;
    // Optional staging: an SRV input buffer and a UAV preload.
    let srv = match srv_data {
        Some(data) => {
            let up = gpu.make(
                D3D12_HEAP_TYPE_UPLOAD,
                &buffer((data.len() * 4) as u64, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
            )?;
            unsafe { write_upload(&up, data) }?;
            let res = gpu.make(
                D3D12_HEAP_TYPE_DEFAULT,
                &buffer((data.len() * 4) as u64, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
            )?;
            Some((up, res))
        }
        None => None,
    };
    let init = match uav_init {
        Some(data) => {
            assert_eq!(
                data.len(),
                uav_dwords as usize,
                "the UAV preload must cover the readback window"
            );
            let up = gpu.make(
                D3D12_HEAP_TYPE_UPLOAD,
                &buffer((data.len() * 4) as u64, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
            )?;
            unsafe { write_upload(&up, data) }?;
            Some(up)
        }
        None => None,
    };
    unsafe {
        let srv_va = srv
            .as_ref()
            .map(|(_, r)| r.GetGPUVirtualAddress())
            .unwrap_or(0);
        let out_va = out.GetGPUVirtualAddress();
        let srv_at = srv.is_some().then(|| view_param(&[cs], 1, 0));
        let uav_at = view_param(&[cs], 2, 0);
        gpu.run(&pso, |list| {
            list.SetComputeRootSignature(&root);
            let mut barriers = Vec::new();
            if let Some((up, res)) = &srv {
                list.CopyBufferRegion(res, 0, up, 0, (srv_data.unwrap().len() * 4) as u64);
                barriers.push(transition(
                    res,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                ));
            }
            if let Some(up) = &init {
                list.CopyBufferRegion(&out, 0, up, 0, (uav_init.unwrap().len() * 4) as u64);
                barriers.push(transition(
                    &out,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                ));
            }
            if !barriers.is_empty() {
                list.ResourceBarrier(&barriers);
            }
            list.SetComputeRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            if let Some(at) = srv_at {
                list.SetComputeRootShaderResourceView(at, srv_va);
            }
            list.SetComputeRootUnorderedAccessView(uav_at, out_va);
            list.Dispatch(groups[0], groups[1], groups[2]);
            list.ResourceBarrier(&[transition(
                &out,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.CopyBufferRegion(&rb, 0, &out, 0, u64::from(uav_dwords) * 4);
        })
    }?;
    unsafe { read_dwords(&rb, uav_dwords as usize) }
}

/// The same compute pipeline dispatched twice in one submission, `u0`
/// rebound to a fresh buffer between the two; returns both images for a
/// bitwise compare.
/// SAFETY: as [`Gpu::run`].
unsafe fn dispatch_twice(
    gpu: &Gpu,
    cs: &HostShader,
    dwords: u32,
    groups: [u32; 3],
) -> Result<(Vec<u32>, Vec<u32>), String> {
    let (root, pso) = gpu.host.compute_pipeline(cs)?;
    let desc = buffer(
        u64::from(dwords) * 4,
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    );
    let a = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &desc,
        D3D12_RESOURCE_STATE_COMMON,
        None,
    )?;
    let b = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &desc,
        D3D12_RESOURCE_STATE_COMMON,
        None,
    )?;
    let rb_desc = buffer(u64::from(dwords) * 4, D3D12_RESOURCE_FLAG_NONE);
    let rb_a = gpu.make(
        D3D12_HEAP_TYPE_READBACK,
        &rb_desc,
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
    )?;
    let rb_b = gpu.make(
        D3D12_HEAP_TYPE_READBACK,
        &rb_desc,
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
    )?;
    let uav_at = view_param(&[cs], 2, 0);
    let (va_a, va_b) = unsafe { (a.GetGPUVirtualAddress(), b.GetGPUVirtualAddress()) };
    unsafe {
        gpu.run(&pso, |list| {
            list.SetComputeRootSignature(&root);
            list.SetComputeRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            list.SetComputeRootUnorderedAccessView(uav_at, va_a);
            list.Dispatch(groups[0], groups[1], groups[2]);
            // Second dispatch, second image; the two write disjoint
            // buffers, so no ordering work is needed between them.
            list.SetComputeRootUnorderedAccessView(uav_at, va_b);
            list.Dispatch(groups[0], groups[1], groups[2]);
            list.ResourceBarrier(&[
                transition(
                    &a,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
                transition(
                    &b,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
            ]);
            list.CopyBufferRegion(&rb_a, 0, &a, 0, u64::from(dwords) * 4);
            list.CopyBufferRegion(&rb_b, 0, &b, 0, u64::from(dwords) * 4);
        })?;
        Ok((
            read_dwords(&rb_a, dwords as usize)?,
            read_dwords(&rb_b, dwords as usize)?,
        ))
    }
}

/// The adapter's description string, for the run log.
fn device_name(dev: &ID3D12Device) -> String {
    unsafe {
        let Ok(factory) = CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) else {
            return String::new();
        };
        let Ok(adapter) = factory.EnumAdapterByLuid::<IDXGIAdapter1>(dev.GetAdapterLuid()) else {
            return String::new();
        };
        let Ok(desc) = adapter.GetDesc1() else {
            return String::new();
        };
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        String::from_utf16_lossy(&desc.Description[..end])
    }
}

/// The shared test prologue: a compiler, a device and a scratch directory.
fn setup(name: &str) -> Option<(PathBuf, Gpu, PathBuf)> {
    let Some(dxc) = find_dxc() else {
        eprintln!("no PC dxc; skipped");
        return None;
    };
    let Ok(gpu) = Gpu::new() else {
        eprintln!("no hardware SM 6.6 device; skipped");
        return None;
    };
    eprintln!("device: {}", device_name(&gpu.host.device));
    let dir = std::env::temp_dir().join(format!("mesia-ca-{name}-{}", std::process::id()));
    Some((dxc, gpu, dir))
}

/// How many of `got`'s lanes disagree with `want`, with the first few
/// printed as `label[index]: got != want`.
fn count_mismatches(got: &[u32], want: &[u32], label: &str) -> usize {
    let mut bad = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if g != w {
            if bad < 8 {
                eprintln!("{label}[{i}]: got {g:#010x} want {w:#010x}");
            }
            bad += 1;
        }
    }
    bad
}

/// The tests' per-thread value: odd multiplier spreads the bits so the
/// high halves differ visibly between neighbours.
fn h(i: u32) -> u32 {
    i.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9)
}

/// Bank-conflict-style groupshared traffic: the values are what matter,
/// not the speed. Thread `gi` of each 256-thread group stores into the
/// stride-32 permutation `32*(gi&7) + (gi>>3)` - eight consecutive lanes
/// land on the same 4-byte bank at different addresses - and into a
/// padded array whose row stride is 33 banks; after a barrier everyone
/// reads slots other lanes wrote, on two rounds of different write
/// permutations. Every slot written is unique, so the CPU can say exactly
/// which lane's value each read must return; a lost store, a race on the
/// barrier, or a banked access that aliased shows as a wrong value.
#[test]
fn groupshared_bank_patterns() {
    const T: usize = 256;
    const G: usize = 8;
    let Some((dxc, gpu, dir)) = setup("banks") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint banked[256];
groupshared uint padded[264]; // 256 lanes + one pad dword every 32
[numthreads(256, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 256u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    // Permutation of 0..255 with eight lanes per bank (stride 32).
    banked[(gi & 7u) * 32u + (gi >> 3u)] = v;
    // Padded row: lane gi sits at gi + gi/32, all distinct.
    padded[gi + (gi >> 5u)] = v ^ 0xA5A5A5A5u;
    GroupMemoryBarrierWithGroupSync();
    uint a = banked[(gi * 8u + 3u) & 255u];
    uint t = (gi * 31u + 3u) & 255u;
    uint b = padded[t + (t >> 5u)];
    GroupMemoryBarrierWithGroupSync();
    // Second round, another permutation of the whole array.
    banked[(gi * 129u + 1u) & 255u] = ~v;
    GroupMemoryBarrierWithGroupSync();
    uint c = banked[(gi * 131u + 9u) & 255u];
    outb.Store4(16u * i, uint4(a, b, c, v));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "banks", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out = unsafe { dispatch_read(&gpu, &cs, (G * T * 4) as u32, [G as u32, 1, 1], |_| {}) }
        .expect("dispatch");
    let mut bad = 0;
    for g in 0..G as u32 {
        for gi in 0..T as u32 {
            let i = g * 256 + gi;
            // Round 1 banked: lane w stored at 32*(w&7)+(w>>3), so slot s
            // was written by w = ((s&31)<<3)|(s>>5).
            let s = (gi * 8 + 3) & 255;
            let w = ((s & 31) << 3) | (s >> 5);
            let want_a = h(g * 256 + w);
            let t = (gi * 31 + 3) & 255;
            let want_b = h(g * 256 + t) ^ 0xA5A5_A5A5;
            // Round 2 banked: lane j stored ~v at (129j+1)&255; 129 is its
            // own inverse mod 256, so slot s2's writer is 129*(s2-1).
            let s2 = (gi * 131 + 9) & 255;
            let w2 = 129u32.wrapping_mul(s2.wrapping_sub(1)) & 255;
            let want_c = !h(g * 256 + w2);
            let want = [want_a, want_b, want_c, h(i)];
            let got = &out[4 * i as usize..4 * i as usize + 4];
            if got != want {
                if bad < 8 {
                    eprintln!("group {g} thread {gi}: got {got:08x?} want {want:08x?}");
                }
                bad += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "groupshared bank-pattern mismatches");
}

/// UAV atomics at maximum contention: 1024 threads all hammer the same
/// eight counters with `InterlockedAdd` (two, one carrying thread ids),
/// `InterlockedMin`/`Max`/`Or`/`And`/`Xor` on the threads' hashed values,
/// then each thread round-trips a private slot through `InterlockedExchange`
/// and wins then deliberately loses an `InterlockedCompareExchange` on a
/// slot preloaded with its thread index. The counters start from CPU-staged
/// initial values (the upload also covers copy->UAV visibility). Every
/// result is order-independent, so the final image is exact.
#[test]
fn uav_atomic_contention() {
    const N: usize = 1024; // 4 groups of 256
    const X0: usize = 16; // exchange slots
    const C0: usize = X0 + N; // compare-exchange slots
    const O0: usize = C0 + N; // last compare-exchange's returned old value
    const DW: usize = O0 + N;
    let Some((dxc, gpu, dir)) = setup("atomics") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(256, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 256u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    outb.InterlockedAdd(0u, v & 0xFFFFu);
    outb.InterlockedAdd(4u, 1u);
    outb.InterlockedMin(8u, v);
    outb.InterlockedMax(12u, v);
    outb.InterlockedOr(16u, v);
    outb.InterlockedAnd(20u, v);
    outb.InterlockedXor(24u, v);
    outb.InterlockedAdd(28u, i);
    uint old;
    outb.InterlockedExchange(4u * (16u + i), ~v, old);
    outb.InterlockedExchange(4u * (16u + i), v, old);
    outb.InterlockedCompareExchange(4u * (1040u + i), i, v, old);
    // The slot now holds v, which is provably never i, so this CAS must
    // fail and report v back.
    outb.InterlockedCompareExchange(4u * (1040u + i), i, 0xDEADBEEFu, old);
    outb.Store(4u * (2064u + i), old);
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "atomics", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let mut init = vec![0u32; DW];
    init[0..8].copy_from_slice(&[0, 0, 0xFFFF_FFFF, 0, 0, 0xFFFF_FFFF, 0, 0]);
    for i in 0..N {
        init[C0 + i] = i as u32;
    }
    // SAFETY: plain D3D12 calls; resources outlive the fence in `run`.
    let out = unsafe { dispatch_full(&gpu, &cs, None, Some(&init), DW as u32, [4, 1, 1]) }
        .expect("dispatch");
    let vals: Vec<u32> = (0..N).map(|i| h(i as u32)).collect();
    let want_ctr = [
        vals.iter().fold(0u32, |a, v| a.wrapping_add(v & 0xFFFF)),
        N as u32,
        vals.iter().copied().min().unwrap(),
        vals.iter().copied().max().unwrap(),
        vals.iter().copied().fold(0, |a, v| a | v),
        vals.iter().copied().fold(!0, |a, v| a & v),
        vals.iter().copied().fold(0, |a, v| a ^ v),
        vals.iter().enumerate().fold(0u32, |a, (i, _)| a + i as u32),
    ];
    let mut want = vec![0u32; DW];
    want[0..8].copy_from_slice(&want_ctr);
    for i in 0..N {
        want[X0 + i] = vals[i];
        want[C0 + i] = vals[i];
        want[O0 + i] = vals[i]; // the second CAS fails, reporting v back
    }
    let bad = count_mismatches(&out, &want, "atomic");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "atomic counter/array values diverged");
}

/// Cross-lane exchange the way wave64->32 lowering emulates a shuffle:
/// every lane of a 64-thread group publishes its value to groupshared,
/// syncs, and reads a permutation - one that crosses the 32-lane halves
/// (the lane pairs a wave32 implementation cannot reach intrinsically),
/// one that stays inside them, plus a rotate. A second round publishes a
/// *computed* value and reads it through a strided map, so correctness
/// depends on both barriers and on the shared image, not on any lane
/// hardware. Exact values throughout.
#[test]
fn groupshared_lane_exchange() {
    const T: usize = 64;
    const G: usize = 8;
    let Some((dxc, gpu, dir)) = setup("xchg") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint sh[64];
[numthreads(64, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 64u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    sh[gi] = v;
    GroupMemoryBarrierWithGroupSync();
    uint rev = sh[63u - gi];       // mirror: farthest lane
    uint xs  = sh[gi ^ 0x20u];     // across the two 32-lane halves
    uint xw  = sh[gi ^ 0x1Fu];     // butterfly inside a half
    uint rot = sh[(gi + 17u) & 63u];
    GroupMemoryBarrierWithGroupSync();
    // Round 2: a value that depends on what round 1 delivered.
    sh[gi] = rev + i;
    GroupMemoryBarrierWithGroupSync();
    uint r2 = sh[(gi * 7u + 11u) & 63u];
    outb.Store(20u * i + 16u, r2);
    outb.Store4(20u * i, uint4(rev, xs, xw, rot));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "xchg", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out = unsafe { dispatch_read(&gpu, &cs, (G * T * 5) as u32, [G as u32, 1, 1], |_| {}) }
        .expect("dispatch");
    let mut bad = 0;
    for g in 0..G as u32 {
        for gi in 0..T as u32 {
            let i = g * 64 + gi;
            let base = g * 64;
            let rev_w = base + (63 - gi);
            // Round-2 slot (7gi+11)&63 was written by its own index.
            let k = (gi * 7 + 11) & 63;
            let want = [
                h(rev_w),
                h(base + (gi ^ 0x20)),
                h(base + (gi ^ 0x1F)),
                h(base + ((gi + 17) & 63)),
                h(base + (63 - k)).wrapping_add(base + k),
            ];
            let got = &out[5 * i as usize..5 * i as usize + 5];
            if got != want {
                if bad < 8 {
                    eprintln!("group {g} lane {gi}: got {got:08x?} want {want:08x?}");
                }
                bad += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "lane-exchange mismatches");
}

/// The `v_mbcnt` pattern: a per-lane predicate is packed into 32-bit mask
/// words by `InterlockedOr` on groupshared, each lane's exclusive prefix
/// is `countbits` of the words below it plus its word's lower bits, and
/// the prefix drives a stream compaction scatter. The mask image itself
/// is dumped for checking, so a dropped bit lands in three places. Exact.
#[test]
fn lane_mask_bitcount() {
    const T: usize = 256;
    const G: usize = 4;
    const N: usize = G * T;
    const QUADS: usize = 4 * N; // [0, 4096)
    const COMP: usize = QUADS; // compaction dump, N dwords
    const MASK: usize = COMP + N; // per-group mask words, G*8 dwords
    const DW: usize = MASK + G * 8;
    let Some((dxc, gpu, dir)) = setup("mbcnt") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint maskw[8];     // the group's 256 lanes, packed
groupshared uint scratch[256]; // the compaction image
[numthreads(256, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 256u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    uint pred = ((v >> 3u) & 7u) == 5u ? 1u : 0u;
    if (gi < 8u) { maskw[gi] = 0u; }
    GroupMemoryBarrierWithGroupSync();
    if (pred != 0u) {
        InterlockedOr(maskw[gi >> 5u], 1u << (gi & 31u));
    }
    GroupMemoryBarrierWithGroupSync();
    uint word = maskw[gi >> 5u];
    uint prefix = countbits(word & ((1u << (gi & 31u)) - 1u));
    for (uint w = 0u; w < (gi >> 5u); ++w) { prefix += countbits(maskw[w]); }
    uint total = 0u;
    for (uint w = 0u; w < 8u; ++w) { total += countbits(maskw[w]); }
    if (pred != 0u) { scratch[prefix] = i; }
    GroupMemoryBarrierWithGroupSync();
    // For pred lanes this reads back the lane's own rank slot: i.
    uint mine = pred != 0u ? scratch[prefix] : 0xDEADBEEFu;
    outb.Store4(16u * i, uint4(word, prefix, total, mine));
    outb.Store(4u * (4096u + i), gi < total ? scratch[gi] : 0xFFFFFFFFu);
    if (gi < 8u) { outb.Store(4u * (5120u + gid.x * 8u + gi), maskw[gi]); }
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "mbcnt", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out =
        unsafe { dispatch_read(&gpu, &cs, DW as u32, [G as u32, 1, 1], |_| {}) }.expect("dispatch");
    let mut want = vec![0u32; DW];
    for g in 0..G {
        let base = g * T;
        let pred: Vec<bool> = (0..T)
            .map(|gi| (h((base + gi) as u32) >> 3) & 7 == 5)
            .collect();
        let mut maskw = [0u32; 8];
        for gi in 0..T {
            if pred[gi] {
                maskw[gi >> 5] |= 1 << (gi & 31);
            }
        }
        let total: u32 = maskw.iter().map(|w| w.count_ones()).sum();
        let lanes: Vec<usize> = (0..T).filter(|&gi| pred[gi]).collect();
        for gi in 0..T {
            let i = base + gi;
            let word = maskw[gi >> 5];
            let prefix = (word & ((1u32 << (gi & 31)) - 1)).count_ones()
                + maskw[..gi >> 5].iter().map(|w| w.count_ones()).sum::<u32>();
            let mine = if pred[gi] { i as u32 } else { 0xDEAD_BEEF };
            want[4 * i..4 * i + 4].copy_from_slice(&[word, prefix, total, mine]);
            want[COMP + i] = if (gi as u32) < total {
                (base + lanes[gi]) as u32
            } else {
                0xFFFF_FFFF
            };
        }
        for w in 0..8 {
            want[MASK + g * 8 + w] = maskw[w];
        }
    }
    let bad = count_mismatches(&out, &want, "mbcnt");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "mask/bitcount or compaction mismatches");
}

/// Read-after-write hazards inside one dispatch. Phase 1 writes each
/// thread's slot and a per-group atomic sum; an
/// `AllMemoryBarrierWithGroupSync` publishes both device-wide, then phase
/// 2 reads another lane's slot and the sum (both must be complete - a
/// weaker barrier leaks a stale or partial value). Phase 3 overwrites the
/// slot behind a second barrier (the WAR edge: phase-2 reads must have
/// landed first) and phase 4 reads a neighbour's new value. The buffer's
/// final image and every thread's reads are checked.
#[test]
fn uav_read_after_write() {
    const T: usize = 256;
    const G: usize = 4;
    const N: usize = G * T;
    const DW: usize = 2048 + 4 * N;
    let Some((dxc, gpu, dir)) = setup("raw") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(256, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 256u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    // Phase 1: plain stores plus one atomic accumulator per group.
    outb.Store(4u * i, v);
    outb.InterlockedAdd(4u * (1024u + gid.x), v & 0xFFFFu);
    AllMemoryBarrierWithGroupSync();
    // Phase 2: another lane's phase-1 value and the finished sum.
    uint got = outb.Load(4u * (gid.x * 256u + ((gi + 13u) & 255u)));
    uint total = outb.Load(4u * (1024u + gid.x));
    AllMemoryBarrierWithGroupSync();
    // Phase 3: overwrite; only the barrier above keeps phase-2 readers
    // from racing this store.
    outb.Store(4u * i, ~v);
    AllMemoryBarrierWithGroupSync();
    // Phase 4: a neighbour's phase-3 value.
    uint got2 = outb.Load(4u * (gid.x * 256u + ((gi + 7u) & 255u)));
    outb.Store4(4u * (2048u + 4u * i), uint4(got, total, got2, v));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "raw", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out =
        unsafe { dispatch_read(&gpu, &cs, DW as u32, [G as u32, 1, 1], |_| {}) }.expect("dispatch");
    let mut want = vec![0u32; DW];
    for i in 0..N {
        want[i] = !h(i as u32); // the phase-3 image
    }
    for g in 0..G {
        want[1024 + g] = (0..T).fold(0u32, |a, gi| {
            a.wrapping_add(h((g * T + gi) as u32) & 0xFFFF)
        });
        for gi in 0..T {
            let i = g * T + gi;
            want[2048 + 4 * i] = h((g * T + ((gi + 13) & 255)) as u32);
            want[2048 + 4 * i + 1] = want[1024 + g];
            want[2048 + 4 * i + 2] = !h((g * T + ((gi + 7) & 255)) as u32);
            want[2048 + 4 * i + 3] = h(i as u32);
        }
    }
    let bad = count_mismatches(&out, &want, "raw");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "read-after-write hazard: stale reads observed");
}

/// Bitwise determinism: the same pipeline dispatched twice in one
/// submission with `u0` rebound to a fresh buffer between the two must
/// produce byte-identical images. The workload mixes a shared-memory tree
/// reduction, float math and per-group atomics - all order-insensitive,
/// so even a scheduler that interleaves the two dispatches differently
/// cannot legitimately differ. Each image is also compared against the
/// CPU reference, so a *deterministically wrong* result still fails.
#[test]
fn dispatch_bitwise_determinism() {
    const T: usize = 128;
    const G: usize = 8;
    const SUMS: usize = 4096;
    const DW: usize = SUMS + G;
    let Some((dxc, gpu, dir)) = setup("det") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint red[128];
[numthreads(128, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 128u + gi;
    uint v = i * 2654435761u + 0x9E3779B9u;
    red[gi] = v;
    GroupMemoryBarrierWithGroupSync();
    for (uint s = 64u; s != 0u; s >>= 1u) {
        if (gi < s) { red[gi] += red[gi + s]; }
        GroupMemoryBarrierWithGroupSync();
    }
    // Deterministic fp: f is in [1, 2), the square is exact IEEE math.
    float f = asfloat(0x3F800000u | (v & 0x7FFFFFu));
    uint w = v ^ red[0] ^ (asuint(f * f) & 0xFFFFFFu);
    outb.Store(4u * i, w);
    outb.InterlockedAdd(4u * (4096u + gid.x), v & 0xFFFFu);
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "det", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    // SAFETY: plain D3D12 calls; every resource outlives the fence.
    let (a, b) =
        unsafe { dispatch_twice(&gpu, &cs, DW as u32, [G as u32, 1, 1]) }.expect("dispatch");
    let mut want = vec![0u32; DW];
    for g in 0..G {
        let base = g * T;
        let sum: u32 = (0..T).fold(0u32, |s, gi| s.wrapping_add(h((base + gi) as u32)));
        for gi in 0..T {
            let i = base + gi;
            let v = h(i as u32);
            let f = f32::from_bits(0x3F80_0000 | (v & 0x7F_FFFF));
            want[i] = v ^ sum ^ ((f * f).to_bits() & 0xFF_FFFF);
            want[SUMS + g] = want[SUMS + g].wrapping_add(v & 0xFFFF);
        }
    }
    let bad = count_mismatches(&a, &want, "det-a") + count_mismatches(&b, &want, "det-b");
    let mut diff = 0;
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            if diff < 8 {
                eprintln!("det[{i}]: run A {x:#010x} != run B {y:#010x}");
            }
            diff += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "dispatch image diverged from the CPU reference");
    assert_eq!(diff, 0, "same dispatch, two runs, different bits");
}

/// f32 -> IEEE binary16, round-to-nearest-even; the CPU oracle for
/// `f32tof16`. Returns the 16-bit pattern.
fn f32_to_f16_bits(x: f32) -> u32 {
    let b = x.to_bits();
    let sign = (b >> 16) & 0x8000;
    let ef = ((b >> 23) & 0xFF) as i32;
    let m = b & 0x7F_FFFF;
    if ef == 0xFF {
        // inf stays inf; nan becomes a quiet nan (payloads may differ).
        return sign | if m == 0 { 0x7C00 } else { 0x7E00 };
    }
    if ef == 0 {
        return sign; // an f32 denormal is far below f16's denorm range
    }
    let e16 = ef - 127 + 15;
    if e16 >= 31 {
        return sign | 0x7C00; // overflow -> inf
    }
    if e16 <= 0 {
        // f16 subnormal, the result counted in units of 2^-24; below
        // e16 == -10 the value cannot reach half of the smallest denorm.
        if e16 < -10 {
            return sign;
        }
        let s = (14 - e16) as u32;
        let sig = m | 0x80_0000;
        // Round-half-even on the shift-out bits.
        let mant = (sig + (1u32 << (s - 1)) - 1 + ((sig >> s) & 1)) >> s;
        return sign | mant; // 0x400 lands on the smallest normal's encoding
    }
    // 23-bit fraction rounded to 10; a carry simply overflows into the
    // exponent field, which is the right encoding.
    let mant = (m + 0xFFF + ((m >> 13) & 1)) >> 13;
    sign | ((e16 as u32) << 10) + mant
}

/// IEEE binary16 -> f32; exact (widening), the oracle for `f16tof32`.
fn f16_to_f32_bits(h: u32) -> u32 {
    let s = (h & 0x8000) << 16;
    let e = (h >> 10) & 0x1F;
    let m = h & 0x3FF;
    if e == 0 {
        if m == 0 {
            return s;
        }
        // Denormal: normalise into an f32 exponent.
        let mut m = m;
        let mut e2 = -14i32;
        while m & 0x400 == 0 {
            m <<= 1;
            e2 -= 1;
        }
        m &= 0x3FF;
        return s | (((e2 + 127) as u32) << 23) | (m << 13);
    }
    if e == 31 {
        return s | 0x7F80_0000 | (m << 13); // inf/nan
    }
    s | (((e as i32 - 15 + 127) as u32) << 23) | (m << 13)
}

/// Half-precision and packed-byte conversions. f32 edge cases (zeros,
/// the f16 overflow midpoint at 65520, denormal boundaries, inf) go
/// through `f32tof16` and back; a table of raw f16 patterns goes through
/// `f16tof32`; `pack_clamp_u8` clamps signed ints into bytes; and
/// `D3DCOLORtoUBYTE4` exercises the float->unorm path. Inputs arrive by
/// SRV so nothing is constant-folded away. Half conversions are exact
/// bits; the unorm bytes allow +-1 against round-to-nearest-even - the
/// intrinsic's rounding is not fully specified - with the observed value
/// printed on failure.
#[test]
fn half_and_unorm_edges() {
    const T: usize = 32;
    const IN_DW: usize = 320;
    const OUT_DW: usize = 10 * T;
    let Some((dxc, gpu, dir)) = setup("half") else {
        return;
    };
    let src = r#"
ByteAddressBuffer inb : register(t0, space1);
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(32, 1, 1)]
void cs(uint gi : SV_GroupIndex) {
    uint bits = inb.Load(4u * gi);
    float x = asfloat(bits);
    uint h = f32tof16(x);
    uint bk = asuint(f16tof32(h));
    uint h2 = inb.Load(4u * (32u + gi)) & 0xFFFFu;
    uint bk2 = asuint(f16tof32(h2));
    float4 u = asfloat(inb.Load4(4u * (64u + 4u * gi)));
    int4 pk4 = asint(inb.Load4(4u * (192u + 4u * gi)));
    uint pk = pack_clamp_u8(pk4);
    uint4 dc = D3DCOLORtoUBYTE4(u);
    outb.Store4(40u * gi, uint4(bits, h, bk, h2));
    outb.Store4(40u * gi + 16u, uint4(bk2, pk, dc.x, dc.y));
    outb.Store2(40u * gi + 32u, uint2(dc.z, dc.w));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "half", src, "cs", "cs_6_6"),
        vec![0],
        vec![0],
    );
    // [0,32) f32 bit patterns; [32,64) f16 patterns; [64,192) unorm
    // float4s; [192,320) int4 pack inputs.
    let mut input = vec![0u32; IN_DW];
    input[0..32].copy_from_slice(&[
        0x0000_0000,
        0x8000_0000,
        0x3F80_0000,
        0xBF80_0000,
        0x3F00_0000,
        0x3EAA_AAAB,
        0x477F_E000,
        0x477F_E800,
        0x477F_F000,
        0x3880_0000,
        0x3380_0000,
        0x3300_0000,
        0x3F80_1C00,
        0x0000_0001,
        0x8000_0001,
        0x7F80_0000,
        0xFF80_0000,
        0x7F7F_FFFF,
        0x4974_2400,
        0xC498_0000,
        0x447A_0000,
        0x3C08_8889,
        0x4EFF_FFFF,
        0x3C00_0000,
        0x3563_9FFF,
        0x3680_0000,
        0x3A00_0000,
        0x42B6_A3D8,
        0xC2B6_A3D8,
        0x4049_0FDB,
        0x3E57_8000,
        0x42DC_7FFF,
    ]);
    input[32..64].copy_from_slice(&[
        0x0000u32, 0x8000, 0x3C00, 0xBC00, 0x0001, 0x03FF, 0x0400, 0x7BFF, 0x7C00, 0xFC00, 0x3555,
        0x4248, 0x7E00, 0x7FFF, 0x1000, 0x2456, 0x2BFF, 0x5A00, 0x0002, 0x0200, 0x1234, 0x5678,
        0x6FFF, 0x7400, 0xAAAB, 0xCD00, 0xF7FF, 0x8100, 0x0FFF, 0x3F6A, 0x4CD3, 0x7150,
    ]);
    for t in 0..T {
        // unorm inputs: edges (below 0, above 1, exactly the halfway
        // points of a byte) plus hash-derived values in [0,1].
        let v = h(t as u32);
        let base = 64 + 4 * t;
        let (a, b, c, d) = match t % 8 {
            0 => (-0.5f32, 0.0, 1.0, 1.5),
            1 => (0.5, 0.25, 0.75, 0.125),
            2 => (0.001, 0.999, 0.1, 0.9),
            3 => (2.0, -2.0, 1.0 / 510.0, 1.0 / 255.0),
            4 => (
                (v & 0xFFFF) as f32 / 65535.0,
                ((v >> 16) & 0xFFFF) as f32 / 65535.0,
                ((v >> 8) & 0xFF) as f32 / 255.0,
                (v & 0xFF) as f32 / 255.0,
            ),
            5 => (0.5019608, 0.49803922, 0.0039, 0.996), // byte halfpoints
            6 => (1.0 / 3.0, 2.0 / 3.0, 0.2, 0.8),
            _ => (0.0, 1.0, 0.5, -0.001),
        };
        input[base..base + 4].copy_from_slice(&[
            a.to_bits(),
            b.to_bits(),
            c.to_bits(),
            d.to_bits(),
        ]);
        let ibase = 192 + 4 * t;
        let (w, x, y, z) = match t % 8 {
            0 => (0i32, 1, 254, 255),
            1 => (256, -1, 128, 127),
            2 => (i32::MAX, i32::MIN, 300, -300),
            3 => (65, 66, 200, 7),
            4 => (-128, 129, 1000, -1000),
            5 => (255, 255, 255, 255),
            6 => ((v as i32) >> 8, ((v >> 3) as i32) << 1, 42, -42),
            _ => (17, 0, 256, -7),
        };
        input[ibase..ibase + 4].copy_from_slice(&[w as u32, x as u32, y as u32, z as u32]);
    }
    // SAFETY: plain D3D12 calls; resources outlive the fence in `run`.
    let out = unsafe { dispatch_full(&gpu, &cs, Some(&input), None, OUT_DW as u32, [1, 1, 1]) }
        .expect("dispatch");
    let mut bad = 0;
    for t in 0..T {
        let row = &out[10 * t..10 * t + 10];
        let (bits, got_h, got_bk, h2, got_bk2, pk) =
            (row[0], row[1], row[2], row[3], row[4], row[5]);
        let dc = &row[6..10];
        let x = f32::from_bits(bits);
        let want_h = f32_to_f16_bits(x);
        // The documented result is RTNE; stacks may instead truncate the
        // mantissa and/or saturate finite overflow to the largest finite
        // (both observed from NVIDIA's f32tof16), which is the same
        // encoding +-1 ulp - including the 0x7BFF/0x7C00 (finite/inf)
        // boundary. Sign and magnitude beyond one ulp are still exact.
        let ulp = |v: u32| (v & 0x7FFF) as i32;
        if got_h & 0x8000 != want_h & 0x8000 || (ulp(got_h) - ulp(want_h)).abs() > 1 {
            if bad < 8 {
                eprintln!("half[{t}]: f32tof16({x:e}) got {got_h:#06x} want {want_h:#06x}");
            }
            bad += 1;
        } else if got_h != want_h {
            eprintln!(
                "half[{t}]: f32tof16({x:e}) = {got_h:#06x}, RTNE {want_h:#06x} (within 1 ulp)"
            );
        }
        // Widening is exact: the round-trip must reproduce whatever the
        // first stage produced, bit for bit.
        let want_bk = f16_to_f32_bits(got_h);
        if got_bk != want_bk {
            if bad < 8 {
                eprintln!(
                    "half[{t}]: f16tof32({got_h:#06x}) got {got_bk:#010x} want {want_bk:#010x}"
                );
            }
            bad += 1;
        }
        let want_bk2 = f16_to_f32_bits(h2 & 0xFFFF);
        let nan = (want_bk2 & 0x7FFF_FFFF) > 0x7F80_0000;
        if nan {
            // NaN payloads may be remapped; only NaN-ness is required.
            if (got_bk2 & 0x7FFF_FFFF) <= 0x7F80_0000 {
                if bad < 8 {
                    eprintln!("half[{t}]: f16tof32({h2:#06x}) got {got_bk2:#010x}, want a NaN");
                }
                bad += 1;
            }
        } else if got_bk2 != want_bk2 {
            if bad < 8 {
                eprintln!(
                    "half[{t}]: f16tof32({h2:#06x}) got {got_bk2:#010x} want {want_bk2:#010x}"
                );
            }
            bad += 1;
        }
        let pk4: Vec<i32> = input[192 + 4 * t..192 + 4 * t + 4]
            .iter()
            .map(|&w| w as i32)
            .collect();
        let want_pk: u32 = pk4
            .iter()
            .enumerate()
            .map(|(k, &v)| (v.clamp(0, 255) as u32) << (8 * k))
            .sum();
        if pk != want_pk {
            if bad < 8 {
                eprintln!("half[{t}]: pack_clamp_u8({pk4:?}) got {pk:#010x} want {want_pk:#010x}");
            }
            bad += 1;
        }
        // D3DCOLORtoUBYTE4, as the stack implements it: a ubyte4 of
        // (u.z, u.y, u.x, u.w) scaled by 255 and truncated toward zero,
        // *not* clamped - out-of-range inputs come back as negative or
        // over-255 ints (e.g. -0.5 -> -127, 1.5 -> 382). +-1 leaves room
        // for a rounding variant over truncation.
        let u4: Vec<f32> = input[64 + 4 * t..64 + 4 * t + 4]
            .iter()
            .map(|&w| f32::from_bits(w))
            .collect();
        let trunc = |c: f32| -> i32 { (c * 255.0).trunc() as i32 };
        for (k, (got, want)) in dc
            .iter()
            .zip([trunc(u4[2]), trunc(u4[1]), trunc(u4[0]), trunc(u4[3])])
            .enumerate()
        {
            if (*got as i32 - want).abs() > 1 {
                if bad < 8 {
                    eprintln!(
                        "half[{t}]: unorm byte {k} got {} want {want} (inputs {u4:?})",
                        *got as i32
                    );
                }
                bad += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "half/unorm conversion mismatches");
}

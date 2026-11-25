//! Compute-side correctness checks for the host D3D12 path: original HLSL
//! compiled at test time with the SDK's `dxc.exe` (the `synth_scene`
//! convention), bound through [`mesia_shader::emit::host::Host`]'s pipeline
//! helpers and dispatched on a hardware device - or on WARP with
//! `MESIA_WARP=1`, the software oracle for whatever the hardware reports.
//!
//! Every shader's output is compared against a CPU-computed reference:
//! integer equality throughout, and exact f32 bits in the interop test
//! (its colours are dyadic fractions, exact in both worlds). The root
//! bindings follow what `Host`'s root signatures declare: 32-bit constants
//! at `b0, space0`, root views `t`/`u`/`b <slot>` in `ROOT_CBV_SPACE`
//! (space 1), and SM 6.6 `ResourceDescriptorHeap[]`/`SamplerDescriptorHeap[]`
//! for heap descriptors.
//!
//! Skipped without a compiler or an SM 6.6 device, like `synth_scene`'s
//! GPU tests.

#![cfg(all(windows, feature = "host"))]

use std::path::{Path, PathBuf};

use mesia_shader::emit::HostShader;
use mesia_shader::emit::host::Host;
use mesia_shader::emit::validator::Validator;
use mesia_shader::HwStage;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::*;
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
            cbvs.chain(srvs).chain(s.root_uavs.iter().map(|&r| (r, 2u8)))
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

fn texture2d(w: u32, h: u32, format: DXGI_FORMAT, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
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



fn options(dev: &ID3D12Device) -> Option<D3D12_FEATURE_DATA_D3D12_OPTIONS> {
    let mut o = D3D12_FEATURE_DATA_D3D12_OPTIONS::default();
    unsafe {
        dev.CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS,
            (&raw mut o).cast(),
            size_of_val(&o) as u32,
        )
    }
    .ok()?;
    Some(o)
}

fn options1(dev: &ID3D12Device) -> Option<D3D12_FEATURE_DATA_D3D12_OPTIONS1> {
    let mut o = D3D12_FEATURE_DATA_D3D12_OPTIONS1::default();
    unsafe {
        dev.CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS1,
            (&raw mut o).cast(),
            size_of_val(&o) as u32,
        )
    }
    .ok()?;
    Some(o)
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
    let dir = std::env::temp_dir().join(format!("mesia-cc-{name}-{}", std::process::id()));
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

/// groupshared memory and `GroupMemoryBarrierWithGroupSync`: 8 groups of
/// 256 threads each write a private value, read a neighbour's across one
/// barrier, then run a 256-wide tree reduction behind a barrier per level.
/// All three results - the own value, the neighbour's and the total, plus
/// the whole final shared state - are checked against a CPU simulation, so
/// a barrier that lost its wait or its memory fence shows up as a stale or
/// partial value somewhere (a missing sync need not show up every run, but
/// the reference is exact where the sync is honoured).
#[test]
fn groupshared_barrier_reduction() {
    const T: usize = 256;
    const G: usize = 8;
    let Some((dxc, gpu, dir)) = setup("shared") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint sh[256];
[numthreads(256, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 256u + gi;
    uint v = i * i + 3u * i + 1u;
    sh[gi] = v;
    GroupMemoryBarrierWithGroupSync();
    uint next = sh[(gi + 1u) & 255u];
    // All the neighbour reads must land before the reduction writes.
    GroupMemoryBarrierWithGroupSync();
    for (uint s = 128u; s != 0u; s >>= 1u) {
        if (gi < s) {
            sh[gi] += sh[gi + s];
        }
        GroupMemoryBarrierWithGroupSync();
    }
    outb.Store4(16u * i, uint4(v, next, sh[0], sh[gi]));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "shared", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out = unsafe { dispatch_read(&gpu, &cs, (G * T * 4) as u32, [G as u32, 1, 1], |_| {}) }
        .expect("dispatch");
    let mut bad = 0;
    for g in 0..G {
        // The group's shared array, and the reduction's end state, on CPU.
        let mut sh = [0u32; T];
        for (j, e) in sh.iter_mut().enumerate() {
            let i = (g * T + j) as u32;
            *e = i * i + 3 * i + 1;
        }
        let next: Vec<u32> = (0..T).map(|j| sh[(j + 1) % T]).collect();
        let mut w = sh;
        let mut s = T / 2;
        while s > 0 {
            for j in 0..s {
                w[j] = w[j].wrapping_add(w[j + s]);
            }
            s >>= 1;
        }
        for j in 0..T {
            let i = g * T + j;
            let got = &out[4 * i..4 * i + 4];
            let want = [sh[j], next[j], w[0], w[j]];
            if got != want {
                if bad < 8 {
                    eprintln!("group {g} thread {j}: got {got:08x?} want {want:08x?}");
                }
                bad += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "groupshared reduction mismatches");
}

/// Wave intrinsics at the device's real wave width: two groups of 32
/// threads, each lane reporting its `WaveGetLaneIndex`, `WaveGetLaneCount`,
/// `WavePrefixSum(lane^2)`, `WaveActiveBallot(true)`,
/// `WaveActiveSum(lane^2)`, `WaveIsFirstLane` and a `WaveReadLaneFirst`
/// broadcast of the wave's lowest lane's thread id. The reference comes
/// from each thread's own ballot, so the checks hold whatever width the
/// device's waves are (the machine's is 32; WARP's is 4).
#[test]
fn wave_prefix_and_broadcast() {
    let Some((dxc, gpu, dir)) = setup("wave") else {
        return;
    };
    let dev = &gpu.host.device;
    let Some(o1) = options1(dev) else {
        eprintln!("D3D12_OPTIONS1 unsupported; skipped");
        return;
    };
    eprintln!(
        "wave lanes {}..={}, wave ops {}",
        o1.WaveLaneCountMin,
        o1.WaveLaneCountMax,
        o1.WaveOps.as_bool()
    );
    if !o1.WaveOps.as_bool() || o1.WaveLaneCountMax == 0 {
        eprintln!("device reports no wave ops; skipped");
        return;
    }
    const N: usize = 64; // two groups of 32.
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(32, 1, 1)]
void cs(uint3 dtid : SV_DispatchThreadID) {
    uint lane = WaveGetLaneIndex();
    uint count = WaveGetLaneCount();
    uint v = lane * lane;
    uint4 row = uint4(lane,
        count | (WaveIsFirstLane() ? 0x80000000u : 0u),
        WavePrefixSum(v),
        WaveReadLaneFirst(dtid.x + 0x10000u));
    outb.Store4(32u * dtid.x, row);
    outb.Store4(32u * dtid.x + 16u, WaveActiveBallot(true));
    outb.Store(4u * (512u + dtid.x), WaveActiveSum(v));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "wave", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out = unsafe { dispatch_read(&gpu, &cs, (8 * N + N) as u32, [2, 1, 1], |_| {}) }
        .expect("dispatch");
    let ballot_of = |row: &[u32]| -> u128 {
        row[4] as u128
            | (row[5] as u128) << 32
            | (row[6] as u128) << 64
            | (row[7] as u128) << 96
    };
    let mut bad = 0;
    for i in 0..N {
        let row = &out[8 * i..8 * i + 8];
        let (lane, countf, prefix, first) = (row[0], row[1], row[2], row[3]);
        let count = countf & 0x7fff_ffff;
        let first_flag = countf & 0x8000_0000 != 0;
        let ballot = ballot_of(row);
        let say = |m: &str, bad: &mut u32| {
            *bad += 1;
            if *bad <= 8 {
                eprintln!("thread {i}: {m}");
            }
        };
        if lane >= count || ballot >> lane & 1 == 0 {
            say(
                &format!("lane {lane} outside count {count}/ballot {ballot:#x}"),
                &mut bad,
            );
            continue;
        }
        if count < o1.WaveLaneCountMin || count > o1.WaveLaneCountMax {
            say(&format!("count {count} outside {o1:?}"), &mut bad);
        }
        // The prefix is the squares of the active lanes below this one.
        let want_pre: u32 = (0..lane)
            .filter(|k| ballot >> k & 1 == 1)
            .map(|k| k * k)
            .sum();
        if prefix != want_pre {
            say(&format!("prefix {prefix:#x} want {want_pre:#x}"), &mut bad);
        }
        if first_flag != (ballot.trailing_zeros() == lane) {
            say(&format!("isfirst {first_flag} at lane {lane} of {ballot:#x}"), &mut bad);
        }
        // The broadcast must come from this wave's lowest active lane:
        // `peer` is that lane's thread row, whose lane must be the ballot's
        // lowest bit and whose ballot the same as this thread's.
        let peer = first.wrapping_sub(0x10000) as usize;
        if peer >= N {
            say(&format!("broadcast {first:#x} not a thread"), &mut bad);
        } else {
            let prow = &out[8 * peer..8 * peer + 8];
            let pb = ballot_of(prow);
            if pb != ballot {
                say(&format!("same-wave ballots differ: {ballot:#x}/{pb:#x}"), &mut bad);
            }
            if prow[0] != pb.trailing_zeros() {
                say(
                    &format!("broadcast from lane {} not lowest {}", prow[0], pb.trailing_zeros()),
                    &mut bad,
                );
            }
        }
        let want_sum: u32 = (0..count)
            .filter(|k| ballot >> k & 1 == 1)
            .map(|k| k * k)
            .sum();
        if out[512 + i] != want_sum {
            say(&format!("active sum {:#x} want {want_sum:#x}", out[512 + i]), &mut bad);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "wave-op mismatches");
}

/// Writes made inside divergent control flow stay where the branch put
/// them: even threads write one groupshared array and the even output
/// slots, odd threads the other array and the odd slots, with a different
/// per-branch loop underneath so the branches genuinely diverge. After the
/// barrier the shared arrays are read back; cross-talk shows as a wrong
/// tag or a wrong writer id, a lost store as a hole.
#[test]
fn divergent_branch_writes() {
    const T: usize = 128;
    const G: usize = 4;
    let Some((dxc, gpu, dir)) = setup("diverge") else {
        return;
    };
    // Output: [0, 512) the divergent direct stores, [512, 1024) the two
    // shared arrays after the barrier, [1024, 1536) the accumulators.
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
groupshared uint sa[64];
groupshared uint sb[64];
[numthreads(128, 1, 1)]
void cs(uint gi : SV_GroupIndex, uint3 gid : SV_GroupID) {
    uint i = gid.x * 128u + gi;
    uint acc;
    [branch]
    if (gi & 1u) {
        acc = 0u;
        for (uint k = 0u; k < (gi & 15u); ++k) {
            acc = acc * 3u + 1u;
        }
        sb[gi >> 1] = 0xB0000000u | i;
        outb.Store(4u * i, 0xDD000000u | i);
    } else {
        acc = 1u;
        for (uint k = 0u; k < (gi & 7u); ++k) {
            acc = acc * 5u + 7u;
        }
        sa[gi >> 1] = 0xA0000000u | i;
        outb.Store(4u * i, 0xEE000000u | i);
    }
    GroupMemoryBarrierWithGroupSync();
    uint base = 512u + gid.x * 128u;
    if (gi < 64u) {
        outb.Store(4u * (base + gi), sa[gi]);
        outb.Store(4u * (base + 64u + gi), sb[gi]);
    }
    outb.Store(4u * (1024u + i), acc);
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "diverge", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let out = unsafe { dispatch_read(&gpu, &cs, (3 * G * T) as u32, [G as u32, 1, 1], |_| {}) }
        .expect("dispatch");
    let mut want = vec![0u32; 3 * G * T];
    for g in 0..G {
        for gi in 0..T {
            let i = g * T + gi;
            if gi & 1 == 1 {
                want[i] = 0xDD00_0000 | i as u32;
                want[512 + g * T + 64 + gi / 2] = 0xB000_0000 | i as u32;
                let mut acc = 0u32;
                for _ in 0..gi & 15 {
                    acc = acc.wrapping_mul(3).wrapping_add(1);
                }
                want[1024 + i] = acc;
            } else {
                want[i] = 0xEE00_0000 | i as u32;
                want[512 + g * T + gi / 2] = 0xA000_0000 | i as u32;
                let mut acc = 1u32;
                for _ in 0..gi & 7 {
                    acc = acc.wrapping_mul(5).wrapping_add(7);
                }
                want[1024 + i] = acc;
            }
        }
    }
    let bad = count_mismatches(&out, &want, "divergent");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "divergent-branch write mismatches");
}

/// The saturation test's descriptor count (inside every binding tier's
/// heap limit: tier 1 allows a million CBV/SRV/UAV descriptors).
const SAT_SRVS: usize = 256;

/// A distinct nonzero constant per descriptor (odd multiplier keeps them
/// injective and spreads them over the word's full range).
fn sat_val(i: u32) -> u32 {
    i.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9)
}

/// The saturation test's device half, as one fenced submission: the 256
/// texture uploads and their state transitions, then the dispatch and its
/// readback. Returns the `SAT_SRVS` indexed reads plus the atomic total.
/// SAFETY: plain D3D12 calls; every resource is alive on this frame until
/// the fence says the GPU is done.
unsafe fn saturation_run(gpu: &Gpu, cs: &HostShader) -> Result<Vec<u32>, String> {
    let dev = &gpu.host.device;
    let (root, pso) = gpu.host.compute_pipeline(cs)?;
    unsafe {
        let views: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: SAT_SRVS as u32,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
            .map_err(err)?;
        let view_size =
            dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
        // One upload buffer, a 256-byte pitch-padded dword per texture.
        let upload = gpu.make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer((SAT_SRVS * 256) as u64, D3D12_RESOURCE_FLAG_NONE),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let mut p = std::ptr::null_mut();
        upload.Map(0, None, Some(&mut p)).map_err(err)?;
        for i in 0..SAT_SRVS {
            (p as *mut u8)
                .add(i * 256)
                .cast::<u32>()
                .write(sat_val(i as u32));
        }
        upload.Unmap(0, None);
        let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        let dwords = (SAT_SRVS + 1) as u32;
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
        let mut textures = Vec::with_capacity(SAT_SRVS);
        gpu.run(&pso, |list| {
            let mut barriers = Vec::with_capacity(SAT_SRVS);
            let mut at = views.GetCPUDescriptorHandleForHeapStart();
            for i in 0..SAT_SRVS {
                let tex = gpu
                    .make(
                        D3D12_HEAP_TYPE_DEFAULT,
                        &texture2d(1, 1, DXGI_FORMAT_R32_UINT, D3D12_RESOURCE_FLAG_NONE),
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        None,
                    )
                    .expect("texture");
                let dst = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&tex),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                };
                let src_loc = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&upload),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                            Offset: (i * 256) as u64,
                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                Format: DXGI_FORMAT_R32_UINT,
                                Width: 1,
                                Height: 1,
                                Depth: 1,
                                RowPitch: 256,
                            },
                        },
                    },
                };
                list.CopyTextureRegion(&dst, 0, 0, 0, &src_loc, None);
                barriers.push(transition(
                    &tex,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                ));
                dev.CreateShaderResourceView(&tex, Some(&srv_desc), at);
                at.ptr += view_size;
                textures.push(tex);
            }
            list.ResourceBarrier(&barriers);
            list.SetDescriptorHeaps(&[Some(views.clone())]);
            list.SetComputeRootSignature(&root);
            list.SetPipelineState(&pso);
            list.SetComputeRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            list.SetComputeRootUnorderedAccessView(
                view_param(&[cs], 2, 0),
                out.GetGPUVirtualAddress(),
            );
            list.Dispatch((SAT_SRVS / 64) as u32, 1, 1);
            list.ResourceBarrier(&[transition(
                &out,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.CopyBufferRegion(&rb, 0, &out, 0, u64::from(dwords) * 4);
        })?;
        read_dwords(&rb, dwords as usize)
    }
}

/// Descriptor-table coverage: 256 `Texture2D` SRVs in one shader-visible
/// heap, each over its own 1x1 `R32_UINT` texture filled with a distinct
/// constant. 256 threads read `ResourceDescriptorHeap[i]` through a
/// `NonUniformResourceIndex` (one dword each plus an atomic total). The
/// binding tier is queried first - the task's "SRV tier" - though every
/// tier is far above 256 descriptors.
#[test]
fn descriptor_table_saturation() {
    let Some((dxc, gpu, dir)) = setup("many") else {
        return;
    };
    let dev = &gpu.host.device;
    let Some(o) = options(dev) else {
        eprintln!("D3D12_OPTIONS unsupported; skipped");
        return;
    };
    eprintln!("resource binding tier: {}", o.ResourceBindingTier.0);
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(64, 1, 1)]
void cs(uint3 dtid : SV_DispatchThreadID) {
    uint i = dtid.x;
    Texture2D<uint4> t = ResourceDescriptorHeap[NonUniformResourceIndex(i)];
    uint v = t.Load(int3(0, 0, 0)).x;
    outb.Store(4u * i, v);
    outb.InterlockedAdd(4u * 256u, v);
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "many", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    // SAFETY: plain D3D12 calls; every resource outlives the fence.
    let out = unsafe { saturation_run(&gpu, &cs) }.expect("dispatch");
    let want: Vec<u32> = (0..SAT_SRVS).map(|i| sat_val(i as u32)).collect();
    let mut bad = count_mismatches(&out[..SAT_SRVS], &want, "srv");
    let sum: u32 = want.iter().fold(0u32, |a, v| a.wrapping_add(*v));
    if out[SAT_SRVS] != sum {
        eprintln!("atomic total: got {:#x} want {sum:#x}", out[SAT_SRVS]);
        bad += 1;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "descriptor-table mismatches");
}

/// The null-descriptor test's device half: view heap 0/1/3 hold null SRVs
/// (a uint texture view, a raw buffer, a float texture view), 2 and 4 live
/// textures with `ctl_load`/`ctl_sample` as their texels, sampler heap 0 a
/// point sampler; the dispatch reads all five.
/// SAFETY: plain D3D12 calls; resources outlive the fence.
unsafe fn null_srv_run(
    gpu: &Gpu,
    cs: &HostShader,
    ctl_load: [f32; 4],
    ctl_sample: [f32; 4],
) -> Result<Vec<u32>, String> {
    let dev = &gpu.host.device;
    unsafe {
        let views: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 5,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
            .map_err(err)?;
        let samplers: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
                NumDescriptors: 1,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
            .map_err(err)?;
        let view_size =
            dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) as usize;
        let at = |i: usize| {
            let mut h = views.GetCPUDescriptorHandleForHeapStart();
            h.ptr += i * view_size;
            h
        };
        let tex_srv = |format: DXGI_FORMAT| D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        // 0: null texture SRV (u32 view); 1: null raw-buffer SRV.
        dev.CreateShaderResourceView(None, Some(&tex_srv(DXGI_FORMAT_R32_UINT)), at(0));
        let buf_srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_TYPELESS,
            ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D12_BUFFER_SRV {
                    NumElements: 16,
                    Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                    ..Default::default()
                },
            },
        };
        dev.CreateShaderResourceView(None, Some(&buf_srv), at(1));
        // 3: null texture SRV (float view, sampled).
        dev.CreateShaderResourceView(
            None,
            Some(&tex_srv(DXGI_FORMAT_R32G32B32A32_FLOAT)),
            at(3),
        );
        // 2 and 4: live 1x1 RGBA32F textures with known texels. Their
        // uploads are copied over inside the dispatch's command list.
        let mut keep = Vec::new();
        for (i, texels) in [(2usize, ctl_load), (4usize, ctl_sample)] {
            let tex = gpu.make(
                D3D12_HEAP_TYPE_DEFAULT,
                &texture2d(
                    1,
                    1,
                    DXGI_FORMAT_R32G32B32A32_FLOAT,
                    D3D12_RESOURCE_FLAG_NONE,
                ),
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
            )?;
            let upload = gpu.make(
                D3D12_HEAP_TYPE_UPLOAD,
                &buffer(256, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
            )?;
            write_upload(&upload, &texels)?;
            dev.CreateShaderResourceView(&tex, Some(&tex_srv(DXGI_FORMAT_R32G32B32A32_FLOAT)), at(i));
            keep.push((tex, upload));
        }
        dev.CreateSampler(
            &D3D12_SAMPLER_DESC {
                Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
                AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
                MinLOD: 0.0,
                MaxLOD: 0.0,
                ..Default::default()
            },
            samplers.GetCPUDescriptorHandleForHeapStart(),
        );
        let out = dispatch_read(gpu, cs, 18, [1, 1, 1], |list| {
            for (tex, upload) in &keep {
                let dst = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(tex),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                };
                let src_loc = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(upload),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                            Offset: 0,
                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                                Width: 1,
                                Height: 1,
                                Depth: 1,
                                RowPitch: 256,
                            },
                        },
                    },
                };
                list.CopyTextureRegion(&dst, 0, 0, 0, &src_loc, None);
            }
            let barriers: Vec<_> = keep
                .iter()
                .map(|(tex, _)| {
                    transition(
                        tex,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                    )
                })
                .collect();
            list.ResourceBarrier(&barriers);
            list.SetDescriptorHeaps(&[Some(views.clone()), Some(samplers.clone())]);
        })?;
        drop(keep);
        Ok(out)
    }
}

/// NULL descriptors: an SRV created with no resource is defined by D3D12
/// to read as zero. Two null views (a `Texture2D` and a raw buffer) are
/// checked next to live controls, plus a `SampleLevel` through a real
/// sampler on a null texture - if the device diverges the observed value
/// is in the assertion text.
#[test]
fn null_srv_descriptor_reads_zero() {
    let Some((dxc, gpu, dir)) = setup("null") else {
        return;
    };
    let src = r#"
RWByteAddressBuffer outb : register(u0, space1);
[numthreads(5, 1, 1)]
void cs(uint3 dtid : SV_DispatchThreadID) {
    if (dtid.x == 0u) {
        Texture2D<uint4> t = ResourceDescriptorHeap[0];
        outb.Store4(0, t.Load(int3(0, 0, 0)));
    } else if (dtid.x == 1u) {
        ByteAddressBuffer b = ResourceDescriptorHeap[1];
        outb.Store(4u * 4u, b.Load(0));
        outb.Store(4u * 5u, b.Load(4));
    } else if (dtid.x == 2u) {
        Texture2D<float4> t = ResourceDescriptorHeap[2];
        outb.Store4(4u * 6u, asuint(t.Load(int3(0, 0, 0))));
    } else if (dtid.x == 3u) {
        Texture2D<float4> t = ResourceDescriptorHeap[3];
        SamplerState s = SamplerDescriptorHeap[0];
        outb.Store4(4u * 10u, asuint(t.SampleLevel(s, float2(0.25, 0.75), 0.0)));
    } else if (dtid.x == 4u) {
        Texture2D<float4> t = ResourceDescriptorHeap[4];
        SamplerState s = SamplerDescriptorHeap[0];
        outb.Store4(4u * 14u, asuint(t.SampleLevel(s, float2(0.75, 0.25), 0.0)));
    }
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "null", src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let ctl_load = [0.5f32, 0.25, 0.125, 1.0];
    let ctl_sample = [1.0f32, 2.0, 4.0, 8.0];
    // SAFETY: plain D3D12 calls; resources outlive the fence.
    let out = unsafe { null_srv_run(&gpu, &cs, ctl_load, ctl_sample) }.expect("dispatch");
    let mut want = vec![0u32; 18];
    want[6..10].copy_from_slice(&ctl_load.map(f32::to_bits));
    want[14..18].copy_from_slice(&ctl_sample.map(f32::to_bits));
    let bad = count_mismatches(&out, &want, "null");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "null-descriptor reads diverged from 0");
}

/// The interop test's device half: the compute dispatch, the
/// UAV→shader-resource transition, the fullscreen draw reading the buffer
/// through a root `ByteAddressBuffer` SRV, and the target's readback - one
/// command list, one fence. Returns the target's texels as raw bits.
/// SAFETY: plain D3D12 calls; everything outlives the fence in `run`.
unsafe fn interop_run(
    gpu: &Gpu,
    cs: &HostShader,
    vs: &HostShader,
    ps: &HostShader,
) -> Result<Vec<u32>, String> {
    const W: u32 = 16;
    const H: u32 = 1;
    let format = DXGI_FORMAT_R32G32B32A32_FLOAT;
    let dev = &gpu.host.device;
    let (croot, cpso) = gpu.host.compute_pipeline(cs)?;
    let (groot, gpso) = gpu.host.pipeline(vs, ps)?;
    let data = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &buffer(
            u64::from(W) * 16,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ),
        D3D12_RESOURCE_STATE_COMMON,
        None,
    )?;
    let rt_clear = D3D12_CLEAR_VALUE {
        Format: format,
        Anonymous: D3D12_CLEAR_VALUE_0 {
            Color: [9.0, 9.0, 9.0, 9.0],
        },
    };
    let target = gpu.make(
        D3D12_HEAP_TYPE_DEFAULT,
        &texture2d(W, H, format, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        Some(&rt_clear),
    )?;
    let pitch = (W as usize * 16).next_multiple_of(256);
    let rb = gpu.make(
        D3D12_HEAP_TYPE_READBACK,
        &buffer((pitch * H as usize) as u64, D3D12_RESOURCE_FLAG_NONE),
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
    )?;
    unsafe {
        let rtvs: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })
            .map_err(err)?;
        let rtv = rtvs.GetCPUDescriptorHandleForHeapStart();
        dev.CreateRenderTargetView(&target, None, rtv);
        let cs_uav = view_param(&[cs], 2, 0);
        let ps_srv = view_param(&[vs, ps], 1, 0);
        let va = data.GetGPUVirtualAddress();
        gpu.run(&cpso, |list| {
            // Compute: 16 threads write the buffer the pixel shader reads.
            list.SetComputeRootSignature(&croot);
            list.SetComputeRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            list.SetComputeRootUnorderedAccessView(cs_uav, va);
            list.Dispatch(1, 1, 1);
            list.ResourceBarrier(&[transition(
                &data,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            )]);
            // Graphics: a fullscreen triangle, one buffer row per pixel.
            list.SetGraphicsRootSignature(&groot);
            list.SetPipelineState(&gpso);
            list.SetGraphicsRoot32BitConstants(0, 1, [0u32].as_ptr().cast(), 0);
            list.SetGraphicsRoot32BitConstants(1, 1, [0u32].as_ptr().cast(), 0);
            list.SetGraphicsRootShaderResourceView(ps_srv, va);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.RSSetViewports(&[D3D12_VIEWPORT {
                Width: W as f32,
                Height: H as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]);
            list.RSSetScissorRects(&[RECT {
                right: W as i32,
                bottom: H as i32,
                ..Default::default()
            }]);
            // The sentinel clear means an unwritten pixel fails rather
            // than matching by luck.
            list.ClearRenderTargetView(rtv, &[9.0f32; 4], None);
            list.DrawInstanced(3, 1, 0, 0);
            list.ResourceBarrier(&[transition(
                &target,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&rb),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: format,
                            Width: W,
                            Height: H,
                            Depth: 1,
                            RowPitch: (W * 16).next_multiple_of(256),
                        },
                    },
                },
            };
            let src_loc = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&target),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &src_loc, None);
        })?;
        read_dwords(&rb, (W * 4) as usize)
    }
}

/// The end-to-end data path: a compute shader fills a raw buffer with
/// dyadic-float colours, a graphics pass reads them through a root
/// `ByteAddressBuffer` SRV and writes them to a 16x1 `RGBA32F` target. The
/// readback's bits must equal the compute shader's exactly - any broken
/// transition or binding in between shows as stale, shifted or zero data.
#[test]
fn compute_to_graphics_interop() {
    let Some((dxc, gpu, dir)) = setup("interop") else {
        return;
    };
    const W: u32 = 16;
    let cs_src = r#"
RWByteAddressBuffer dst : register(u0, space1);
[numthreads(16, 1, 1)]
void cs(uint3 dtid : SV_DispatchThreadID) {
    uint i = dtid.x;
    float4 c = float4(i / 16.0, (i * i) / 256.0, 1.0 - i / 16.0, 1.0);
    dst.Store4(16u * i, asuint(c));
}
"#;
    let gfx_src = r#"
ByteAddressBuffer src : register(t0, space1);
float4 vs(uint vid : SV_VertexID) : SV_Position {
    return float4(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0, 0.5, 1.0);
}
float4 ps(float4 pos : SV_Position) : SV_Target {
    uint i = (uint)pos.x;
    return asfloat(src.Load4(16u * i));
}
"#;
    let cs = shader(
        HwStage::Cs,
        compile(&dxc, &dir, "interp-cs", cs_src, "cs", "cs_6_6"),
        Vec::new(),
        vec![0],
    );
    let vs = shader(
        HwStage::Vs,
        compile(&dxc, &dir, "interp-gfx", gfx_src, "vs", "vs_6_6"),
        Vec::new(),
        Vec::new(),
    );
    let ps = shader(
        HwStage::Ps,
        compile(&dxc, &dir, "interp-gfx", gfx_src, "ps", "ps_6_6"),
        vec![0],
        Vec::new(),
    );
    // SAFETY: plain D3D12 calls; everything created outlives the fence in
    // `run` (the list records both passes in one submission).
    let out = unsafe { interop_run(&gpu, &cs, &vs, &ps) }.expect("interop");
    let want: Vec<u32> = (0..W)
        .flat_map(|i| {
            let (i, i2) = (i as f32, (i * i) as f32);
            [i / 16.0, i2 / 256.0, 1.0 - i / 16.0, 1.0].map(f32::to_bits)
        })
        .collect();
    let bad = count_mismatches(&out, &want, "interop");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(bad, 0, "compute-to-graphics data path mismatches");
}

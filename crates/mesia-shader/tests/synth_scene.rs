//! Synthetic graphics-integrity scene: a deterministic multi-pass draw on a
//! real or software (`Host::new`) D3D12 device, then a byte-level readback
//! check. This is the reference renderer the translation layer's output is
//! later compared against: three draw calls into one target (an indexed
//! triangle-fan rect, an 8x8 instanced quad grid with a per-instance colour,
//! and a second target written by a fullscreen pass and composited back into
//! a corner of the main one), then a second frame drawn after resource data
//! was changed, to catch stale resource/buffer state.
//!
//! Shaders are small original HLSL sources (below) compiled with the SDK's
//! `dxc.exe` (`MESIA_PC_DXC`, else the newest Windows SDK's, next to
//! `dxil.dll`) and bound through [`mesia_shader::emit::host::Host`]'s
//! pipeline helpers; the command recording, resources and readback are this
//! file's (the crate's draw helpers are `cfg(test)` inside it).
//!
//! Determinism: every drawn colour is a flat shader-computed value or an
//! exact texture `Load` - no filtering, blending, or interpolated varyings
//! (the grid colour is `nointerpolation`), and every primitive edge lies on
//! an integer pixel boundary, and computed colours divide by powers of two
//! only (a `/ 7` came back one ulp off on hardware), so frame 1 is
//! bit-identical on WARP and on hardware. `GOLDEN_*` are FNV-1a sums of
//! the readback, measured; the per-pixel region asserts beside them carry
//! the check on their own where a driver disagrees on an edge case.
//!
//! Skipped without a hardware device or a compiler, like `emit`'s GPU
//! tests (WARP: see the test).

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
use windows::core::Interface;

/// The HLSL for the scene's four vertex and three pixel entries (one file,
/// compiled per entry). `vs_fan`/`vs_grid`/`ps_solid` draw into the main
/// target, `vs_full`/`ps_tex` into the second, `vs_corner`/`ps_blit`
/// composite it back. `ps_tex`/`ps_blit` read their source image from the
/// shader-visible heap (`ResourceDescriptorHeap`, as translated shaders
/// do): index 1 is the uploaded 4x4 texture, index 0 the second target.
const HLSL: &str = r#"
struct Flat {
    float4 pos : SV_Position;
    nointerpolation float4 col : COLOR0;
};

// Fullscreen rect drawn as an indexed triangle fan; positions come in as
// NDC pairs, the colour is per-vertex but equal at all four corners.
Flat vs_fan(float2 pos : POSITION, float4 col : COLOR0) {
    Flat o;
    o.pos = float4(pos, 0.5, 1.0);
    o.col = col;
    return o;
}

// One 6x6 px quad per instance, laid out on an 8 px grid over the 64x64
// target; the colour is a function of the instance id only.
Flat vs_grid(float2 corner : POSITION, uint iid : SV_InstanceID) {
    Flat o;
    uint ix = iid % 8u, iy = iid / 8u;
    float2 px = float2(ix, iy) * 8.0 + 1.0 + corner * 6.0;
    o.pos = float4(px.x / 32.0 - 1.0, 1.0 - px.y / 32.0, 0.5, 1.0);
    o.col = float4((iid % 8u) / 8.0, (iid / 8u) / 8.0, (iid % 4u) / 4.0, 1.0);
    return o;
}

// A fullscreen triangle for the second target; no vertex input.
float4 vs_full(uint vid : SV_VertexID) : SV_Position {
    return float4(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0, 0.5, 1.0);
}

// The composite quad: pixels [48,64) x [48,64) of the main target.
float4 vs_corner(float2 corner : POSITION) : SV_Position {
    float2 px = 48.0 + corner * 16.0;
    return float4(px.x / 32.0 - 1.0, 1.0 - px.y / 32.0, 0.5, 1.0);
}

float4 ps_solid(Flat i) : SV_Target {
    return i.col;
}

// Second target: a blocky 8x magnification of the uploaded 4x4 texture.
float4 ps_tex(float4 pos : SV_Position) : SV_Target {
    Texture2D<float4> t = ResourceDescriptorHeap[1];
    return t.Load(int3(int2(pos.xy) / 8, 0));
}

// Composite: the second target at 2x into the main target's corner.
float4 ps_blit(float4 pos : SV_Position) : SV_Target {
    Texture2D<float4> b = ResourceDescriptorHeap[0];
    return b.Load(int3((int2(pos.xy) - 48) * 2, 0));
}
"#;

const W: u32 = 64;
const H: u32 = 64;
const BW: u32 = 32;
const BH: u32 = 32;
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R32G32B32A32_FLOAT;

/// Frame-1 fan colour; frame 2 repaints the same vertices `FAN_B`.
const FAN_A: [f32; 4] = [0.125, 0.25, 0.5, 1.0];
const FAN_B: [f32; 4] = [0.75, 0.25, 0.125, 1.0];

/// The 4x4 source texture's texels for each frame (`texels[ty * 4 + tx]`).
/// Frame 2 gives every texel a new value: if the composite corner keeps the
/// old colours the resource update was not seen (the stale-data class of
/// bug this harness exists to catch).
fn texels(frame: u32) -> Vec<[f32; 4]> {
    (0..16u32)
        .map(|i| {
            let (tx, ty) = ((i % 4) as f32, (i / 4) as f32);
            match frame {
                0 => [(tx + 1.0) / 4.0, (ty + 1.0) / 4.0, ((i + i / 4) % 4) as f32 / 3.0, 1.0],
                _ => [
                    1.0 - (tx + 1.0) / 4.0,
                    1.0 - (ty + 1.0) / 4.0,
                    (tx * ty / 3.0 + 1.0) / 4.0,
                    1.0,
                ],
            }
        })
        .collect()
}

/// The instanced grid colour the shaders compute (see `vs_grid`).
fn grid(iid: u32) -> [f32; 4] {
    [
        (iid % 8) as f32 / 8.0,
        (iid / 8) as f32 / 8.0,
        (iid % 4) as f32 / 4.0,
        1.0,
    ]
}

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}

/// `dxc.exe` next to the SDK `dxil.dll` [`Validator::default_path`] finds,
/// or `MESIA_PC_DXC` (the same convention as `emit`'s diff tests).
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
    let src = dir.join("synth.hlsl");
    std::fs::write(&src, HLSL).unwrap();
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

/// A dxc-compiled container as the host pipeline helpers take it: no user
/// data, samplers, or bound views (the shaders read only heap descriptors).
fn shader(stage: HwStage, dxil: Vec<u8>) -> HostShader {
    HostShader {
        stage,
        dxil,
        user_data: Vec::new(),
        samplers: Vec::new(),
        static_samplers: Vec::new(),
        root_cbvs: Vec::new(),
        root_srvs: Vec::new(),
        root_uavs: Vec::new(),
        root_indexed_views: Vec::new(),
    }
}

/// The device-side scene: both render targets, the source texture and its
/// upload, the shared index buffer and the two vertex buffers, the view
/// heap (0: second target's SRV, 1: source texture's SRV) and the readback.
struct Scene {
    host: Host,
    queue: ID3D12CommandQueue,
    rtv_a: D3D12_CPU_DESCRIPTOR_HANDLE,
    rtv_b: D3D12_CPU_DESCRIPTOR_HANDLE,
    target_a: ID3D12Resource,
    target_b: ID3D12Resource,
    texture: ID3D12Resource,
    upload_tex: ID3D12Resource,
    vb_fan: ID3D12Resource,
    vb_quad: ID3D12Resource,
    ib: ID3D12Resource,
    views: ID3D12DescriptorHeap,
    readback: ID3D12Resource,
    keep: Vec<ID3D12Resource>,
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

fn texture2d(w: u32, h: u32, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(w),
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: FORMAT,
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

impl Scene {
    /// SAFETY: plain D3D12 calls whose descriptions and uploads outlive
    /// them; everything created stays owned by `self` until the GPU is
    /// done with it (the fence in [`Scene::render`]).
    unsafe fn new(host: Host) -> Result<Self, String> {
        let dev = &host.device;
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
            Format: FORMAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: c },
        };
        let target_a = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &texture2d(W, H, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            Some(&rt_clear([0.0; 4])),
        )?;
        let target_b = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &texture2d(BW, BH, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            Some(&rt_clear([0.0; 4])),
        )?;
        let texture = make(
            D3D12_HEAP_TYPE_DEFAULT,
            &texture2d(4, 4, D3D12_RESOURCE_FLAG_NONE),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        let pitch = (4 * 16usize).next_multiple_of(256);
        let upload_tex = make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer((pitch * 4) as u64),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let vb_fan = make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer(4 * 24),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let vb_quad = make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer(4 * 8),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let ib = make(
            D3D12_HEAP_TYPE_UPLOAD,
            &buffer(6 * 2),
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        )?;
        let rtvs: ID3D12DescriptorHeap = unsafe {
            dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 2,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let views: ID3D12DescriptorHeap = unsafe {
            dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })
        }
        .map_err(err)?;
        let rtv_size = unsafe {
            dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV)
        } as usize;
        let view_size = unsafe {
            dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
        } as usize;
        let rtv_a = unsafe { rtvs.GetCPUDescriptorHandleForHeapStart() };
        let mut rtv_b = rtv_a;
        rtv_b.ptr += rtv_size;
        unsafe {
            dev.CreateRenderTargetView(&target_a, None, rtv_a);
            dev.CreateRenderTargetView(&target_b, None, rtv_b);
        }
        let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: FORMAT,
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
        unsafe { dev.CreateShaderResourceView(&target_b, Some(&srv), at) };
        at.ptr += view_size;
        unsafe { dev.CreateShaderResourceView(&texture, Some(&srv), at) };
        let pitch_a = (W as usize * 16).next_multiple_of(256);
        let readback = make(
            D3D12_HEAP_TYPE_READBACK,
            &buffer((pitch_a * H as usize) as u64),
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
        )?;
        // The unit corner quad the grid and the composite pass share, and
        // the triangle-fan indices (two triangles of a four-vertex fan).
        let quad: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        unsafe { write_upload(&vb_quad, &quad)? };
        unsafe { write_upload(&ib, &indices)? };
        Ok(Scene {
            host,
            queue,
            rtv_a,
            rtv_b,
            target_a,
            target_b,
            texture,
            upload_tex,
            vb_fan,
            vb_quad,
            ib,
            views,
            readback,
            keep: Vec::new(),
        })
    }

    /// One frame: upload the source texture, clear the main target, draw
    /// the fan then the instanced grid, draw the fullscreen pass into the
    /// second target, composite it into the corner, read the main target
    /// back. `again` marks a repeat submission, restoring the states the
    /// previous frame left.
    fn render(
        &self,
        pipes: &Pipes,
        fan: [f32; 4],
        texels: &[[f32; 4]],
        again: bool,
    ) -> Result<Vec<[f32; 4]>, String> {
        let dev = &self.host.device;
        // The fan's vertices: an NDC rect plus the frame's colour.
        let mut fan_verts = [0.0f32; 4 * 6];
        for (v, p) in fan_verts
            .chunks_exact_mut(6)
            .zip([(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)])
        {
            v[0] = p.0;
            v[1] = p.1;
            v[2..6].copy_from_slice(&fan);
        }
        unsafe { write_upload(&self.vb_fan, &fan_verts)? };
        // The texture upload rows are pitch-padded.
        let pitch = (4 * 16usize).next_multiple_of(256);
        unsafe {
            let mut p = std::ptr::null_mut();
            self.upload_tex.Map(0, None, Some(&mut p)).map_err(err)?;
            for (y, row) in texels.chunks_exact(4).enumerate() {
                std::ptr::copy_nonoverlapping(
                    row.as_ptr().cast::<u8>(),
                    (p as *mut u8).add(y * pitch),
                    64,
                );
            }
            self.upload_tex.Unmap(0, None);
        }
        // SAFETY: descriptions and uploads outlive the recording; the
        // fence wait below keeps every resource alive until the GPU has
        // finished the frame.
        unsafe {
            let alloc: ID3D12CommandAllocator =
                dev.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                    .map_err(err)?;
            let list: ID3D12GraphicsCommandList = dev
                .CreateCommandList(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    &alloc,
                    &pipes.fan.1,
                )
                .map_err(err)?;
            if again {
                list.ResourceBarrier(&[
                    transition(
                        &self.target_a,
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                    transition(
                        &self.target_b,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                    transition(
                        &self.texture,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                    ),
                ]);
            }
            let subresource = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(r),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let footprint = |r: &ID3D12Resource, w: u32, h: u32| {
                D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(r),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                            Offset: 0,
                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                Format: FORMAT,
                                Width: w,
                                Height: h,
                                Depth: 1,
                                RowPitch: (w * 16).next_multiple_of(256),
                            },
                        },
                    },
                }
            };
            list.CopyTextureRegion(
                &subresource(&self.texture),
                0,
                0,
                0,
                &footprint(&self.upload_tex, 4, 4),
                None,
            );
            list.ResourceBarrier(&[transition(
                &self.texture,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            )]);
            list.SetDescriptorHeaps(&[Some(self.views.clone())]);
            let viewport = |w: u32, h: u32| D3D12_VIEWPORT {
                Width: w as f32,
                Height: h as f32,
                MaxDepth: 1.0,
                ..Default::default()
            };
            let scissor = |w: u32, h: u32| RECT {
                right: w as i32,
                bottom: h as i32,
                ..Default::default()
            };
            let pass = |pipe: &(ID3D12RootSignature, ID3D12PipelineState),
                        vb: Option<(&ID3D12Resource, u32, u32)>| {
                list.SetGraphicsRootSignature(&pipe.0);
                list.SetPipelineState(&pipe.1);
                list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                if let Some((buf, size, stride)) = vb {
                    list.IASetVertexBuffers(
                        0,
                        Some(&[D3D12_VERTEX_BUFFER_VIEW {
                            BufferLocation: buf.GetGPUVirtualAddress(),
                            SizeInBytes: size,
                            StrideInBytes: stride,
                        }]),
                    );
                    list.IASetIndexBuffer(Some(&D3D12_INDEX_BUFFER_VIEW {
                        BufferLocation: self.ib.GetGPUVirtualAddress(),
                        SizeInBytes: 12,
                        Format: DXGI_FORMAT_R16_UINT,
                    }));
                }
            };
            list.OMSetRenderTargets(1, Some(&self.rtv_a), false, None);
            list.RSSetViewports(&[viewport(W, H)]);
            list.RSSetScissorRects(&[scissor(W, H)]);
            list.ClearRenderTargetView(self.rtv_a, &[0.0f32; 4], None);
            // 1: the indexed fan rect.
            pass(&pipes.fan, Some((&self.vb_fan, 4 * 24, 24)));
            list.DrawIndexedInstanced(6, 1, 0, 0, 0);
            // 2: the 8x8 instanced quad grid over it.
            pass(&pipes.grid, Some((&self.vb_quad, 4 * 8, 8)));
            list.DrawIndexedInstanced(6, 64, 0, 0, 0);
            // 3: the fullscreen pass into the second target.
            list.OMSetRenderTargets(1, Some(&self.rtv_b), false, None);
            list.RSSetViewports(&[viewport(BW, BH)]);
            list.RSSetScissorRects(&[scissor(BW, BH)]);
            list.ClearRenderTargetView(self.rtv_b, &[0.0f32; 4], None);
            pass(&pipes.full, None);
            list.DrawInstanced(3, 1, 0, 0);
            // 4: the corner composite (second target as a shader resource).
            list.ResourceBarrier(&[transition(
                &self.target_b,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            )]);
            list.OMSetRenderTargets(1, Some(&self.rtv_a), false, None);
            list.RSSetViewports(&[viewport(W, H)]);
            list.RSSetScissorRects(&[scissor(W, H)]);
            pass(&pipes.corner, Some((&self.vb_quad, 4 * 8, 8)));
            list.DrawIndexedInstanced(6, 1, 0, 0, 0);
            list.ResourceBarrier(&[transition(
                &self.target_a,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.CopyTextureRegion(
                &footprint(&self.readback, W, H),
                0,
                0,
                0,
                &subresource(&self.target_a),
                None,
            );
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
            let mut p = std::ptr::null_mut();
            self.readback.Map(0, None, Some(&mut p)).map_err(err)?;
            let pitch_a = (W as usize * 16).next_multiple_of(256);
            let mut out = Vec::with_capacity((W * H) as usize);
            for y in 0..H as usize {
                out.extend_from_slice(std::slice::from_raw_parts(
                    (p as *const u8).add(y * pitch_a).cast::<[f32; 4]>(),
                    W as usize,
                ));
            }
            self.readback.Unmap(0, None);
            let _ = &self.keep;
            Ok(out)
        }
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
                let bytes = std::slice::from_raw_parts((*m).pDescription, (*m).DescriptionByteLength.saturating_sub(1));
                text.push_str("
  ");
                text.push_str(&String::from_utf8_lossy(bytes));
            }
        }
    }
    text
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

struct Pipes {
    fan: (ID3D12RootSignature, ID3D12PipelineState),
    grid: (ID3D12RootSignature, ID3D12PipelineState),
    full: (ID3D12RootSignature, ID3D12PipelineState),
    corner: (ID3D12RootSignature, ID3D12PipelineState),
}

/// FNV-1a over the readback's raw texel bytes.
fn hash(image: &[[f32; 4]]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for texel in image {
        for f in texel {
            for b in f.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    h
}

/// The golden FNV-1a sums of both frames, measured on an RTX 3060 (frame 1
/// also on WARP, bit-identical) with the 10.0.26100 SDK dxc, ps/vs 6.6; the per-pixel asserts
/// below remain the load-bearing check where another driver differs.
const GOLDEN_FRAME_1: u64 = 0x1967_4552_0ce3_4ae5;
const GOLDEN_FRAME_2: u64 = 0x0db0_7851_e86f_8b25;

#[test]
fn synthetic_scene_renders_and_refreshes() {
    let Some(dxc) = find_dxc() else {
        eprintln!("no PC dxc; skipped");
        return;
    };
    // Hardware only: on WARP the second frame's submission faults inside
    // the software rasterizer (an open issue — the same commands pass on
    // hardware, bit-identical frame 1 on both). `MESIA_SYNTH_WARP=1` runs
    // it there anyway, to investigate.
    let warp = std::env::var_os("MESIA_WARP").is_some_and(|v| v == "1");
    if warp && std::env::var_os("MESIA_SYNTH_WARP").is_none_or(|v| v != "1") {
        eprintln!("WARP: second frame faults there (open issue); skipped");
        return;
    }
    let Ok(host) = Host::new(false) else {
        eprintln!("no hardware SM 6.6 device; skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("mesia-synth-{}", std::process::id()));
    let compile = |entry: &str| {
        self::compile(
            &dxc,
            &dir,
            entry,
            if entry.starts_with("vs") {
                "vs_6_6"
            } else {
                "ps_6_6"
            },
        )
    };
    let vs_fan = shader(HwStage::Vs, compile("vs_fan"));
    let vs_grid = shader(HwStage::Vs, compile("vs_grid"));
    let vs_full = shader(HwStage::Vs, compile("vs_full"));
    let vs_corner = shader(HwStage::Vs, compile("vs_corner"));
    let ps_solid = shader(HwStage::Ps, compile("ps_solid"));
    let ps_tex = shader(HwStage::Ps, compile("ps_tex"));
    let ps_blit = shader(HwStage::Ps, compile("ps_blit"));
    let pipes = Pipes {
        fan: host.pipeline(&vs_fan, &ps_solid).expect("fan pipeline"),
        grid: host.pipeline(&vs_grid, &ps_solid).expect("grid pipeline"),
        full: host.pipeline(&vs_full, &ps_tex).expect("fullscreen pipeline"),
        corner: host
            .pipeline(&vs_corner, &ps_blit)
            .expect("composite pipeline"),
    };
    let scene = unsafe { Scene::new(host) }.expect("scene");
    let at = |img: &Vec<[f32; 4]>, x: u32, y: u32| img[(y * W + x) as usize];

    // Frame 1: the fixed scene.
    let tex0 = texels(0);
    let img1 = scene
        .render(&pipes, FAN_A, &tex0, false)
        .expect("frame 1");
    assert_eq!(img1.len(), (W * H) as usize, "readback size");
    assert!(
        img1.iter().all(|p| p[3] == 1.0),
        "coverage: every texel written (alpha 1)"
    );
    let distinct: std::collections::BTreeSet<[u32; 4]> =
        img1.iter().map(|p| p.map(f32::to_bits)).collect();
    assert!(distinct.len() > 60, "coverage: 64 grid colours plus more");
    // Region checks (pixel centers, integer-boundary geometry: exact).
    assert_eq!(at(&img1, 0, 0), FAN_A, "fan gap top-left");
    assert_eq!(at(&img1, 8, 7), FAN_A, "fan gap inside the grid");
    assert_eq!(at(&img1, 1, 1), grid(0), "grid cell 0");
    assert_eq!(at(&img1, 27, 44), grid(5 * 8 + 3), "grid cell (3,5)");
    assert_eq!(at(&img1, 46, 46), grid(5 * 8 + 5), "grid cell (5,5)");
    assert_eq!(at(&img1, 50, 50), tex0[0], "composite corner top-left");
    assert_eq!(at(&img1, 62, 62), tex0[15], "composite corner bottom-right");
    assert_eq!(at(&img1, 56, 52), tex0[4 + 2], "composite mid");
    let h1 = hash(&img1);
    assert_eq!(h1, GOLDEN_FRAME_1, "frame 1 golden hash ({h1:#x} != expected)");

    // Frame 2: same scene after the fan's colours and the source texture
    // were rewritten - new pixels must appear where the data changed and
    // nowhere else (the stale-resource check).
    let tex1 = texels(1);
    let img2 = scene
        .render(&pipes, FAN_B, &tex1, true)
        .unwrap_or_else(|e| panic!("frame 2: {e}{}", debug_messages(&scene.host.device)));
    assert_eq!(img2.len(), img1.len());
    let unchanged = img1
        .iter()
        .zip(&img2)
        .filter(|(a, b)| a == b)
        .count();
    // The instanced-quad pixels are the only ones that should match,
    // less the four cells (6..8, 6..8) the composite corner covers.
    assert_eq!(unchanged, (64 - 4) * 6 * 6, "only the grid pixels stay");
    assert_eq!(at(&img2, 0, 0), FAN_B, "fan repainted");
    assert_eq!(at(&img2, 1, 1), grid(0), "grid unchanged");
    assert_eq!(at(&img2, 50, 50), tex1[0], "composite sees new texel");
    assert_eq!(at(&img2, 62, 62), tex1[15]);
    let h2 = hash(&img2);
    assert_eq!(h2, GOLDEN_FRAME_2, "frame 2 golden hash ({h2:#x} != expected)");

    let _ = std::fs::remove_dir_all(&dir);
}

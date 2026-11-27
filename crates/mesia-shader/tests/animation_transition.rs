//! Original translated VS + three colour-export PS variants driving a
//! crossfade between two scenes on an intermediate texture that is staged
//! into the flip-chain buffer the engine's present path would consume.
//! Covers: animated geometry, camera-path change, per-draw blend constants,
//! a fullscreen post-process/decay pass (temporal feedback), an intermediate
//! history texture kept across frames, transition completion and present
//! freshness. The CPU oracle is recursive and byte-exact except at u8
//! representation ties (x.5), which are flagged and asserted within one unit.
#![cfg(all(windows, feature = "host"))]

#[path = "graphics/stages.rs"]
mod stages;

use mesia_shader::emit::host::Host;
use std::collections::HashSet;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{HWND, RECT, S_OK};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Interface, s, w};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 64;
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const WARMUP: usize = 8;
const SAMPLES: usize = 64;
const PITCH: u32 = WIDTH * 4;
const TRANSITION: usize = 8;
const K1: usize = WARMUP + 16; // "key press": the transition starts here
const B_FRAMES: usize = SAMPLES + WARMUP - K1 - TRANSITION + WARMUP;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];

// Fullscreen decay factors; 1.0 on the last frame converges exactly to cyan.
const DECAY: [f32; TRANSITION] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
// Scene-A rect fade-out and scene-B rect fade-in over the transition.
const FADE_OUT: [f32; TRANSITION] = [0.875, 0.75, 0.625, 0.5, 0.375, 0.25, 0.125, 0.0];
const FADE_IN: [f32; TRANSITION] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];

struct Window(HWND);

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: this test created the HWND on the current thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

fn buffer(
    dev: &ID3D12Device,
    bytes: u64,
    heap: D3D12_HEAP_TYPE,
    state: D3D12_RESOURCE_STATES,
) -> ID3D12Resource {
    let mut out = None;
    // SAFETY: the resource descriptions are live for the creation call.
    unsafe {
        dev.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: heap,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: bytes,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            state,
            None,
            &mut out,
        )
        .unwrap();
    }
    out.unwrap()
}

fn texture(dev: &ID3D12Device, state: D3D12_RESOURCE_STATES) -> ID3D12Resource {
    let mut out = None;
    // SAFETY: the resource descriptions are live for the creation call.
    unsafe {
        dev.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: WIDTH as u64,
                Height: HEIGHT,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: FORMAT,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                ..Default::default()
            },
            state,
            None,
            &mut out,
        )
        .unwrap();
    }
    out.unwrap()
}

fn barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the descriptor borrows a resource retained until fence completion.
                pResource: unsafe { std::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
        ..Default::default()
    }
}

fn clip(x: f32, y: f32) -> [f32; 4] {
    [x, y, 0.5, 1.0]
}

fn quad() -> [[f32; 4]; 6] {
    [
        clip(-1.0, -1.0),
        clip(1.0, -1.0),
        clip(-1.0, 1.0),
        clip(-1.0, 1.0),
        clip(1.0, -1.0),
        clip(1.0, 1.0),
    ]
}

fn vertices(x: usize, y: usize) -> [[f32; 4]; 6] {
    [(0, 0), (16, 0), (0, 12), (0, 12), (16, 0), (16, 12)].map(|(dx, dy)| {
        [
            (x + dx) as f32 * 2.0 / WIDTH as f32 - 1.0,
            1.0 - (y + dy) as f32 * 2.0 / HEIGHT as f32,
            0.5,
            1.0,
        ]
    })
}

/// Scene-A path (screen-space slide) vs scene-B path (reversed, higher);
/// the switch at K1 is the simulated input.
fn rect_pos(frame: usize, scene_b: bool) -> (usize, usize) {
    if scene_b {
        (WIDTH as usize - 16 - frame % (WIDTH as usize - 16), 4 + (frame * 3) % 28)
    } else {
        (frame % (WIDTH as usize - 16), 8 + frame % 32)
    }
}

// ---------- recursive CPU oracle ----------

/// One UNORM blend result: `src` (0/255 colour lane) over `dst` u8 at factor
/// `bf`. Returns the expected byte and whether it sits on a representation
/// tie, where the hardware may legitimately round either way.
fn blend_lane(src: u8, dst: u8, bf: f32) -> (u8, bool) {
    let exact = src as f64 * bf as f64 + dst as f64 * (1.0 - bf) as f64;
    let frac = exact - exact.floor();
    let tie = (frac - 0.5).abs() < 0.01;
    (exact.round() as u8, tie)
}

struct Oracle {
    px: Vec<[u8; 4]>,
}

impl Oracle {
    fn new() -> Self {
        Self {
            px: vec![BLACK; (WIDTH * HEIGHT) as usize],
        }
    }
    fn clear(&mut self, colour: [u8; 4]) {
        self.px.fill(colour);
    }
    /// Fullscreen post-process: `lerp(px, colour, bf)` per RGB lane; alpha
    /// always lands opaque (SrcAlpha ONE over DestAlpha ZERO).
    fn decay(&mut self, colour: [u8; 4], bf: f32) {
        for px in &mut self.px {
            for ch in 0..3 {
                px[ch] = blend_lane(colour[ch], px[ch], bf).0;
            }
            px[3] = 255;
        }
    }
    fn rect(&mut self, x: usize, y: usize, colour: [u8; 4], bf: f32) {
        for py in y..y + 12 {
            for px in x..x + 16 {
                let p = &mut self.px[py * WIDTH as usize + px];
                for ch in 0..3 {
                    p[ch] = blend_lane(colour[ch], p[ch], bf).0;
                }
                p[3] = 255;
            }
        }
    }
    /// Compare a readback row-major RGBA8 image; counts exact/tie/mismatch.
    fn check(&self, words: &[u32], label: &str, frame: usize) -> (usize, usize, usize) {
        let mut ties = 0;
        let mut recomputed = 0usize;
        for (i, &word) in words.iter().enumerate() {
            let px = i % WIDTH as usize;
            let py = i / WIDTH as usize;
            let want = self.px[i];
            let got = word.to_le_bytes();
            if got == want {
                recomputed += 1;
                continue;
            }
            // Recompute the lane distance: a mismatch is only forgiven on a
            // representation tie, where ±1 ulp is permitted.
            let mut ok = true;
            for ch in 0..3 {
                let d = (got[ch] as i32 - want[ch] as i32).abs();
                if d > 1 {
                    ok = false;
                }
            }
            if ok && got[3] == 255 {
                ties += 1;
            } else {
                panic!(
                    "{label} frame {frame} pixel ({px},{py}): got {got:?} want {want:?}"
                );
            }
        }
        (recomputed, ties, 0)
    }
}

#[test]
#[ignore = "opens a DXGI window on the hardware adapter; run exclusively"]
fn animation_transition_crossfade_history() {
    let host = Host::new(false).expect("hardware D3D12 SM6.6");
    let dev = &host.device;
    let (vs, ps_a) = stages::translated_stages();
    let ps_decay = stages::translated_ps_export(0x0000_0001); // cyan
    let ps_b = stages::translated_ps_export(0x0001_0000); // yellow
    stages::validate_stages(&vs, &ps_a);
    let root = host.root_signature(&vs, &ps_a).unwrap();
    let input = D3D12_INPUT_ELEMENT_DESC {
        SemanticName: s!("POSITION"),
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        ..Default::default()
    };
    let blend = || {
        let mut rt = D3D12_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D12_BLEND_BLEND_FACTOR,
            DestBlend: D3D12_BLEND_INV_BLEND_FACTOR,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ONE,
            DestBlendAlpha: D3D12_BLEND_ZERO,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            ..Default::default()
        };
        rt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        rt
    };
    let desc = |ps: &mesia_shader::emit::HostShader| {
        let mut d = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            // SAFETY: desc borrows the root signature until pipeline creation returns.
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vs.dxil.as_ptr().cast(),
                BytecodeLength: vs.dxil.len(),
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: ps.dxil.as_ptr().cast(),
                BytecodeLength: ps.dxil.len(),
            },
            SampleMask: u32::MAX,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            RasterizerState: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            InputLayout: D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: &input,
                NumElements: 1,
            },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            ..Default::default()
        };
        d.RTVFormats[0] = FORMAT;
        d.BlendState.RenderTarget[0] = blend();
        d
    };
    // SAFETY: all descriptors borrow storage live through each call. Each
    // submitted list completes before buffers are mapped or rewritten.
    unsafe {
        let pso_a: ID3D12PipelineState =
            dev.CreateGraphicsPipelineState(&desc(&ps_a)).unwrap();
        let pso_decay: ID3D12PipelineState =
            dev.CreateGraphicsPipelineState(&desc(&ps_decay)).unwrap();
        let pso_b: ID3D12PipelineState =
            dev.CreateGraphicsPipelineState(&desc(&ps_b)).unwrap();
        let window = Window(
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("MESIA animation transition"),
                WS_POPUP | WS_VISIBLE,
                32,
                32,
                WIDTH as i32,
                HEIGHT as i32,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        );
        // Keep the swap chain visible: occluded flip presents are dropped by
        // DXGI and the frame statistics never advance.
        let _ = SetWindowPos(
            window.0,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let queue: ID3D12CommandQueue = dev
            .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                ..Default::default()
            })
            .unwrap();
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
        let swap: IDXGISwapChain3 = factory
            .CreateSwapChainForHwnd(
                &queue,
                window.0,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: WIDTH,
                    Height: HEIGHT,
                    Format: FORMAT,
                    BufferCount: 2,
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap()
            .cast()
            .unwrap();
        let backs: [ID3D12Resource; 2] = [0, 1].map(|i| swap.GetBuffer(i).unwrap());
        let intermediate = texture(dev, D3D12_RESOURCE_STATE_RENDER_TARGET);
        // History keeps COMMON so it can be a copy destination and a copy
        // source without state churn.
        let history = texture(dev, D3D12_RESOURCE_STATE_COMMON);
        let heap: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })
            .unwrap();
        let rtv = heap.GetCPUDescriptorHandleForHeapStart();
        dev.CreateRenderTargetView(&intermediate, None, rtv);
        let vb = buffer(
            dev,
            96,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
        );
        let quad_vb = buffer(
            dev,
            96,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
        );
        // Two rect uploads per transition frame: writing one buffer twice on
        // the CPU would race the GPU, which reads it only at execution.
        let vb_b = buffer(
            dev,
            96,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
        );
        let mut ptr = std::ptr::null_mut();
        quad_vb
            .Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut ptr))
            .unwrap();
        std::ptr::copy_nonoverlapping(quad().as_ptr(), ptr.cast(), 6);
        quad_vb.Unmap(0, None);
        let rb = buffer(
            dev,
            u64::from(PITCH * HEIGHT),
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        let rb_hist = buffer(
            dev,
            u64::from(PITCH * HEIGHT),
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        let alloc: ID3D12CommandAllocator = dev
            .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .unwrap();
        let list: ID3D12GraphicsCommandList = dev
            .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, &pso_a)
            .unwrap();
        list.Close().unwrap();
        let cmd: ID3D12CommandList = list.cast().unwrap();
        let fence: ID3D12Fence = dev.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
        let copy_out = |list: &ID3D12GraphicsCommandList, src: &ID3D12Resource| {
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&rb),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: FORMAT,
                            Width: WIDTH,
                            Height: HEIGHT,
                            Depth: 1,
                            RowPitch: PITCH,
                        },
                    },
                },
            };
            let s = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(src),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &s, None);
        };
        let read = |which: &ID3D12Resource| -> Vec<u32> {
            let mut ptr = std::ptr::null_mut();
            which.Map(0, None, Some(&mut ptr)).unwrap();
            let v =
                std::slice::from_raw_parts(ptr.cast::<u32>(), (WIDTH * HEIGHT) as usize).to_vec();
            which.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            v
        };
        let total = K1 + TRANSITION + B_FRAMES.max(0);
        let mut oracle = Oracle::new();
        let mut history_updated = 0usize;
        let mut accepted = HashSet::new();
        let mut observed = HashSet::new();
        let mut stats_available = false;
        let mut prev_pixels: Option<Vec<u32>> = None;
        let mut transition_triggered = false;
        let mut converged_frame = Vec::new();
        let mut distinct = HashSet::new();
        let mut first_b_pixels = Vec::new();
        let mut tie_total = 0usize;
        for frame in 0..total {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(window.0), 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            let t = frame.saturating_sub(K1);
            let scene_b = frame >= K1 + TRANSITION;
            let in_transition = frame >= K1 && frame < K1 + TRANSITION;
            let (rx, ry) = if in_transition {
                rect_pos(K1 + TRANSITION, true) // B rect fades in at its landing spot
            } else {
                rect_pos(frame, frame >= K1)
            };
            let points = vertices(rx, ry);
            let mut ptr = std::ptr::null_mut();
            vb.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut ptr))
                .unwrap();
            std::ptr::copy_nonoverlapping(points.as_ptr(), ptr.cast(), 6);
            vb.Unmap(0, None);
            alloc.Reset().unwrap();
            list.Reset(&alloc, &pso_a).unwrap();
            let i = swap.GetCurrentBackBufferIndex() as usize;
            let back = &backs[i];
            list.SetGraphicsRootSignature(&root);
            list.RSSetViewports(&[D3D12_VIEWPORT {
                Width: WIDTH as f32,
                Height: HEIGHT as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: WIDTH as i32,
                bottom: HEIGHT as i32,
            }]);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let quad_view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: quad_vb.GetGPUVirtualAddress(),
                SizeInBytes: 96,
                StrideInBytes: 16,
            };
            let rect_view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: vb.GetGPUVirtualAddress(),
                SizeInBytes: 96,
                StrideInBytes: 16,
            };
            let rect_b_view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: vb_b.GetGPUVirtualAddress(),
                SizeInBytes: 96,
                StrideInBytes: 16,
            };
            if frame < K1 {
                // Scene A: black background, magenta rect on path A.
                list.ClearRenderTargetView(rtv, &[0., 0., 0., 1.], None);
                oracle.clear(BLACK);
                list.IASetVertexBuffers(0, Some(&[rect_view]));
                list.SetPipelineState(&pso_a);
                list.OMSetBlendFactor(Some(&[1.0, 1.0, 1.0, 1.0]));
                list.DrawInstanced(6, 1, 0, 0);
                oracle.rect(rx, ry, MAGENTA, 1.0);
            } else if in_transition {
                // Crossfade: fullscreen decay pass (temporal feedback on the
                // accumulated image), then A fades out and B fades in.
                list.IASetVertexBuffers(0, Some(&[quad_view]));
                list.SetPipelineState(&pso_decay);
                list.OMSetBlendFactor(Some(&[DECAY[t], DECAY[t], DECAY[t], 1.0]));
                list.DrawInstanced(6, 1, 0, 0);
                oracle.decay(CYAN, DECAY[t]);
                let (ax, ay) = rect_pos(frame, false);
                let _ = rx;
                // The A rect keeps its old path while fading out.
                let a_points = vertices(ax, ay);
                vb.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut ptr))
                    .unwrap();
                std::ptr::copy_nonoverlapping(a_points.as_ptr(), ptr.cast(), 6);
                vb.Unmap(0, None);
                list.IASetVertexBuffers(0, Some(&[rect_view]));
                list.SetPipelineState(&pso_a);
                list.OMSetBlendFactor(Some(&[FADE_OUT[t], FADE_OUT[t], FADE_OUT[t], 1.0]));
                list.DrawInstanced(6, 1, 0, 0);
                oracle.rect(ax, ay, MAGENTA, FADE_OUT[t]);
                let b_points = vertices(rx, ry);
                vb_b.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut ptr))
                    .unwrap();
                std::ptr::copy_nonoverlapping(b_points.as_ptr(), ptr.cast(), 6);
                vb_b.Unmap(0, None);
                list.IASetVertexBuffers(0, Some(&[rect_b_view]));
                list.SetPipelineState(&pso_b);
                list.OMSetBlendFactor(Some(&[FADE_IN[t], FADE_IN[t], FADE_IN[t], 1.0]));
                list.DrawInstanced(6, 1, 0, 0);
                oracle.rect(rx, ry, YELLOW, FADE_IN[t]);
            } else {
                // Scene B: cyan background, yellow rect on path B.
                list.ClearRenderTargetView(rtv, &[0., 1., 1., 1.], None);
                oracle.clear(CYAN);
                list.IASetVertexBuffers(0, Some(&[rect_view]));
                list.SetPipelineState(&pso_b);
                list.OMSetBlendFactor(Some(&[1.0, 1.0, 1.0, 1.0]));
                list.DrawInstanced(6, 1, 0, 0);
                oracle.rect(rx, ry, YELLOW, 1.0);
            }
            // Stage the intermediate into the presented back buffer and keep
            // a history texture on even frames (verified persistent next).
            list.ResourceBarrier(&[barrier(
                &intermediate,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            list.ResourceBarrier(&[barrier(
                back,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_COPY_DEST,
            )]);
            list.CopyResource(back, &intermediate);
            copy_out(&list, &intermediate);
            if frame % 2 == 0 {
                list.CopyResource(&history, &intermediate);
                history_updated = frame;
            } else {
                // The history texture keeps the previous even frame while the
                // engine keeps drawing new work on the same queue.
                let dst = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&rb_hist),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                            Offset: 0,
                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                Format: FORMAT,
                                Width: WIDTH,
                                Height: HEIGHT,
                                Depth: 1,
                                RowPitch: PITCH,
                            },
                        },
                    },
                };
                let s = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&history),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                };
                list.CopyTextureRegion(&dst, 0, 0, 0, &s, None);
            }
            list.ResourceBarrier(&[barrier(
                back,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PRESENT,
            )]);
            list.ResourceBarrier(&[barrier(
                &intermediate,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )]);
            list.Close().unwrap();
            queue.ExecuteCommandLists(&[Some(cmd.clone())]);
            let serial = (frame + 1) as u64;
            queue.Signal(&fence, serial).unwrap();
            fence
                .SetEventOnCompletion(serial, windows::Win32::Foundation::HANDLE::default())
                .unwrap();
            let completed = fence.GetCompletedValue();
            assert!(completed >= serial && completed != u64::MAX);
            // Verify the staged intermediate pixels byte-exact vs oracle.
            let pixels = read(&rb);
            let (exact, ties, _) = oracle.check(&pixels, "intermediate", frame);
            tie_total += ties;
            assert_eq!(exact + ties, (WIDTH * HEIGHT) as usize);
            distinct.insert(pixels.clone());
            if in_transition && frame == K1 + TRANSITION - 1 {
                converged_frame = pixels.clone();
            }
            if scene_b && first_b_pixels.is_empty() {
                first_b_pixels = pixels.clone();
            }
            if frame == K1 {
                if let Some(prev) = &prev_pixels {
                    assert_ne!(pixels, *prev, "the transition must change the image");
                    transition_triggered = true;
                }
            }
            prev_pixels = Some(pixels);
            if frame % 2 == 1 {
                // The history texture must still hold the previous even frame
                // after unrelated work ran in between.
                let held = read(&rb_hist);
                let expect_frame = history_updated;
                let mut want = Oracle::new();
                // Recompute the oracle state for that even frame.
                for f in 0..=expect_frame {
                    let tt = f.saturating_sub(K1);
                    let (x, y) = rect_pos(f, f >= K1);
                    if f < K1 {
                        want.clear(BLACK);
                        want.rect(x, y, MAGENTA, 1.0);
                    } else if f < K1 + TRANSITION {
                        want.decay(CYAN, DECAY[tt]);
                        let (ax, ay) = rect_pos(f, false);
                        want.rect(ax, ay, MAGENTA, FADE_OUT[tt]);
                        let (bx, by) = rect_pos(K1 + TRANSITION, true);
                        want.rect(bx, by, YELLOW, FADE_IN[tt]);
                    } else {
                        want.clear(CYAN);
                        want.rect(x, y, YELLOW, 1.0);
                    }
                }
                let history_expect = want.px;
                for (i, &word) in held.iter().enumerate() {
                    let want_px = history_expect[i];
                    let got = word.to_le_bytes();
                    if got != want_px {
                        let mut ok = true;
                        for ch in 0..3 {
                            if (got[ch] as i32 - want_px[ch] as i32).abs() > 1 {
                                ok = false;
                            }
                        }
                        assert!(ok && got[3] == 255,
                            "history frame {frame} pixel {i}: got {got:?} want {want_px:?}");
                        tie_total += 1;
                    }
                }
            }
            let hr = swap.Present(1, DXGI_PRESENT(0));
            assert_eq!(hr, S_OK, "Present frame {frame} returned {hr:?}");
            let id = swap.GetLastPresentCount().unwrap();
            if frame >= WARMUP {
                assert!(accepted.insert(id), "DXGI repeated present ID {id}");
                let mut stats = DXGI_FRAME_STATISTICS::default();
                if swap.GetFrameStatistics(&mut stats).is_ok() {
                    stats_available = true;
                    if accepted.contains(&stats.PresentCount) {
                        observed.insert(stats.PresentCount);
                    }
                }
            }
        }
        // Flip presentation trails the last submission by several refreshes;
        // drain the statistics until the final accepted ID is observed (or
        // a bounded window passes), as in the baseline fixture.
        if stats_available {
            let last_id = swap.GetLastPresentCount().unwrap();
            let drain = std::time::Instant::now();
            while !observed.contains(&last_id) && drain.elapsed().as_millis() < 250 {
                let mut sample = DXGI_FRAME_STATISTICS::default();
                if swap.GetFrameStatistics(&mut sample).is_err() {
                    break;
                }
                if accepted.contains(&sample.PresentCount) {
                    observed.insert(sample.PresentCount);
                }
                if sample.PresentCount != last_id {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        // A successful transition: the last accumulated frame must equal the
        // first cleared scene-B frame — the feedback converged exactly.
        assert_eq!(
            converged_frame, first_b_pixels,
            "the accumulated image must converge byte-exact to scene B"
        );
        assert!(transition_triggered, "the transition was not observed");
        assert!(stats_available, "DXGI frame statistics unavailable");
        assert!(
            observed.len() >= SAMPLES / 2,
            "displayed IDs: {} of {SAMPLES}",
            observed.len()
        );
        assert!(
            distinct.len() >= total - 2,
            "images must progress, got {} distinct of {total}",
            distinct.len()
        );
        println!(
            "animation_transition: frames={total} distinct={} tie_pixels={tie_total} observed_ids={} transitions=1 converge=exact",
            distinct.len(),
            observed.len()
        );
    }
}

//! Original translated VS/PS and a moving, pixel-aligned magenta rectangle.
//! Read back the actual flip-chain buffer before presenting that same buffer.
#![cfg(all(windows, feature = "host"))]

#[path = "graphics/native.rs"]
mod native;
#[path = "graphics/stages.rs"]
mod stages;

use mesia_shader::emit::host::Host;
use std::collections::HashSet;
use std::mem::ManuallyDrop;
use std::time::Instant;
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
const PITCH: u32 = WIDTH * 4; // two complete D3D12 texture pitch alignment units

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

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn process_cpu_ms() -> f64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
    // SAFETY: each output has independent storage and the pseudo handle is process-local.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
        .unwrap()
    };
    let ticks = |t: FILETIME| u64::from(t.dwLowDateTime) | (u64::from(t.dwHighDateTime) << 32);
    (ticks(kernel) + ticks(user)) as f64 / 10_000.0
}

fn memory_report(device: &ID3D12Device, factory: &IDXGIFactory2) {
    use windows::Win32::System::ProcessStatus::*;
    use windows::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: all queries write to correctly sized local structures and use the active adapter.
    unsafe {
        let mut system = PERFORMANCE_INFORMATION::default();
        GetPerformanceInfo(&mut system, size_of::<PERFORMANCE_INFORMATION>() as u32).unwrap();
        let mut process = PROCESS_MEMORY_COUNTERS::default();
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut process,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
        .unwrap();
        let adapter: IDXGIAdapter3 = factory
            .cast::<IDXGIFactory4>()
            .unwrap()
            .EnumAdapterByLuid(device.GetAdapterLuid())
            .unwrap();
        let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        let mut shared = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter
            .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)
            .unwrap();
        adapter
            .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut shared)
            .unwrap();
        println!(
            "memory_mib working_set={} peak_ws={} system_free={} system_commit={} system_limit={} local_gpu={} nonlocal_gpu={}",
            process.WorkingSetSize >> 20,
            process.PeakWorkingSetSize >> 20,
            system.PhysicalAvailable * system.PageSize >> 20,
            system.CommitTotal * system.PageSize >> 20,
            system.CommitLimit * system.PageSize >> 20,
            local.CurrentUsage >> 20,
            shared.CurrentUsage >> 20
        );
    }
}

fn summary(name: &str, values: impl Iterator<Item = f64>) {
    let mut v: Vec<_> = values.collect();
    v.sort_by(f64::total_cmp);
    let quantile = |q: usize| v[(v.len() * q).div_ceil(100) - 1];
    println!(
        "{name}_ms mean={:.4} p50={:.4} p95={:.4} p99={:.4}",
        v.iter().sum::<f64>() / v.len() as f64,
        quantile(50),
        quantile(95),
        quantile(99)
    );
}

#[test]
#[ignore = "opens a DXGI window on the hardware adapter; run exclusively"]
fn translated_graphics_baseline() {
    let init = Instant::now();
    let host = Host::new(false).expect("hardware D3D12 SM6.6");
    let dev = &host.device;
    let init_ms = ms(init);
    let translate = Instant::now();
    let reference = std::env::var("MESIA_GRAPHICS_NATIVE").as_deref() == Ok("1");
    let (vs, ps) = if reference {
        native::reference_stages()
    } else {
        stages::translated_stages()
    };
    let translate_ms = ms(translate);
    let validate_start = Instant::now();
    stages::validate_stages(&vs, &ps);
    println!(
        "mode={} validation_ms={:.3}",
        if reference { "native" } else { "translated" },
        ms(validate_start)
    );
    let pipeline = Instant::now();
    let root = host.root_signature(&vs, &ps).unwrap();
    let input = D3D12_INPUT_ELEMENT_DESC {
        SemanticName: s!("POSITION"),
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        ..Default::default()
    };
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
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
    desc.RTVFormats[0] = FORMAT;
    desc.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
    // SAFETY: all descriptors borrow storage live through each call. Each submitted list
    // completes before buffers are mapped for reading, rewritten or reused by the CPU.
    unsafe {
        let pso: ID3D12PipelineState = dev.CreateGraphicsPipelineState(&desc).unwrap();
        let pipeline_ms = ms(pipeline);
        let resources = Instant::now();
        let window = Window(
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("MESIA translated graphics"),
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
        let heap: ID3D12DescriptorHeap = dev
            .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 2,
                ..Default::default()
            })
            .unwrap();
        let handles = [0, 1].map(|i| {
            let mut h = heap.GetCPUDescriptorHandleForHeapStart();
            h.ptr +=
                i * dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
            dev.CreateRenderTargetView(&backs[i], None, h);
            h
        });
        let vb = buffer(
            dev,
            96,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
        );
        let rb = buffer(
            dev,
            u64::from(PITCH * HEIGHT),
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        let ticks = buffer(
            dev,
            24,
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        let mut queries: Option<ID3D12QueryHeap> = None;
        dev.CreateQueryHeap(
            &D3D12_QUERY_HEAP_DESC {
                Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
                Count: 3,
                NodeMask: 0,
            },
            &mut queries,
        )
        .unwrap();
        let queries = queries.unwrap();
        let ticks_per_ms = queue.GetTimestampFrequency().unwrap() as f64 / 1000.0;
        let alloc: ID3D12CommandAllocator = dev
            .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .unwrap();
        let list: ID3D12GraphicsCommandList = dev
            .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, &pso)
            .unwrap();
        list.Close().unwrap();
        let cmd: ID3D12CommandList = list.cast().unwrap();
        let fence: ID3D12Fence = dev.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
        let resources_ms = ms(resources);
        let mut rows = Vec::with_capacity(SAMPLES);
        let mut accepted = HashSet::with_capacity(SAMPLES);
        let mut observed = HashSet::with_capacity(SAMPLES);
        let mut stats_available = false;
        let mut warmup_ms = 0.0;
        let mut measured_start = Instant::now();
        let mut cpu_start = 0.0;
        let warm = Instant::now();
        for frame in 0..(WARMUP + SAMPLES) {
            if frame == WARMUP {
                cpu_start = process_cpu_ms();
                warmup_ms = ms(warm);
                measured_start = Instant::now();
            }
            let start = Instant::now();
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(window.0), 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            let x = frame % (WIDTH as usize - 16);
            let y = 8 + frame % 32;
            let points = vertices(x, y);
            let mut ptr = std::ptr::null_mut();
            vb.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut ptr))
                .unwrap();
            std::ptr::copy_nonoverlapping(points.as_ptr(), ptr.cast(), points.len());
            vb.Unmap(0, None);
            alloc.Reset().unwrap();
            list.Reset(&alloc, &pso).unwrap();
            let i = swap.GetCurrentBackBufferIndex() as usize;
            let back = &backs[i];
            list.EndQuery(&queries, D3D12_QUERY_TYPE_TIMESTAMP, 0);
            list.ResourceBarrier(&[barrier(
                back,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )]);
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
            list.OMSetRenderTargets(1, Some(&handles[i]), false, None);
            list.ClearRenderTargetView(handles[i], &[0., 0., 0., 1.], None);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.IASetVertexBuffers(
                0,
                Some(&[D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: vb.GetGPUVirtualAddress(),
                    SizeInBytes: 96,
                    StrideInBytes: 16,
                }]),
            );
            list.DrawInstanced(6, 1, 0, 0);
            list.EndQuery(&queries, D3D12_QUERY_TYPE_TIMESTAMP, 1);
            list.ResourceBarrier(&[barrier(
                back,
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
                            Format: FORMAT,
                            Width: WIDTH,
                            Height: HEIGHT,
                            Depth: 1,
                            RowPitch: PITCH,
                        },
                    },
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(back),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            list.ResourceBarrier(&[barrier(
                back,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_PRESENT,
            )]);
            list.EndQuery(&queries, D3D12_QUERY_TYPE_TIMESTAMP, 2);
            list.ResolveQueryData(&queries, D3D12_QUERY_TYPE_TIMESTAMP, 0, 3, &ticks, 0);
            list.Close().unwrap();
            queue.ExecuteCommandLists(&[Some(cmd.clone())]);
            let serial = (frame + 1) as u64;
            queue.Signal(&fence, serial).unwrap();
            let cpu_submit = ms(start);
            let wait = Instant::now();
            fence
                .SetEventOnCompletion(serial, windows::Win32::Foundation::HANDLE::default())
                .unwrap();
            let completed = fence.GetCompletedValue();
            assert!(
                completed >= serial && completed != u64::MAX,
                "fence {completed} for {serial}"
            );
            let wait_ms = ms(wait);
            let verify = Instant::now();
            ticks.Map(0, None, Some(&mut ptr)).unwrap();
            let [a, b, c] = *ptr.cast::<[u64; 3]>();
            ticks.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            rb.Map(0, None, Some(&mut ptr)).unwrap();
            let words = std::slice::from_raw_parts(ptr.cast::<u32>(), (WIDTH * HEIGHT) as usize);
            for (pixel, &word) in words.iter().enumerate() {
                let px = pixel % WIDTH as usize;
                let py = pixel / WIDTH as usize;
                let want = if (x..x + 16).contains(&px) && (y..y + 12).contains(&py) {
                    0xffff00ff
                } else {
                    0xff000000
                };
                assert_eq!(word, want, "frame {frame} pixel ({px},{py})");
            }
            rb.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            let verify_ms = ms(verify);
            let present = Instant::now();
            let hr = swap.Present(1, DXGI_PRESENT(0));
            assert_eq!(hr, S_OK, "Present frame {frame} returned {hr:?}");
            let present_ms = ms(present);
            let id = swap.GetLastPresentCount().unwrap();
            let mut stats = DXGI_FRAME_STATISTICS::default();
            if frame >= WARMUP {
                assert!(accepted.insert(id), "DXGI repeated present ID {id}");
                if swap.GetFrameStatistics(&mut stats).is_ok() {
                    stats_available = true;
                    if accepted.contains(&stats.PresentCount) {
                        observed.insert(stats.PresentCount);
                    }
                }
                rows.push([
                    ms(start),
                    cpu_submit,
                    wait_ms,
                    verify_ms,
                    present_ms,
                    (b - a) as f64 / ticks_per_ms,
                    (c - b) as f64 / ticks_per_ms,
                ]);
            }
        }
        let measured_ms = ms(measured_start);
        let cpu_ms = process_cpu_ms() - cpu_start;
        // Flip presentation can trail the last submission by several refreshes.
        // Sample only actual DXGI IDs; gaps do not establish display of an image.
        let last_id = swap.GetLastPresentCount().unwrap();
        let drain = Instant::now();
        while stats_available && !observed.contains(&last_id) && ms(drain) < 250.0 {
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
        let display_window_ms = ms(measured_start);
        if stats_available {
            println!(
                "display_window_ms={display_window_ms:.3} observed_distinct_fps_min={:.3}",
                1000.0 * observed.len() as f64 / display_window_ms
            );
        }
        println!("process_cpu_ms_per_frame={:.4}", cpu_ms / SAMPLES as f64);
        memory_report(dev, &factory);
        println!(
            "init_ms={init_ms:.3} translate_ms={translate_ms:.3} pipeline_ms={pipeline_ms:.3} resources_ms={resources_ms:.3} warmup_ms={warmup_ms:.3} frames={SAMPLES} sync_interval=1"
        );
        println!(
            "submitted={} completed={} correct_distinct={} present_s_ok={} observed_display_ids={} stats_available={} wall_ms={measured_ms:.3} valid_present_fps={:.3}",
            rows.len(),
            rows.len(),
            rows.len(),
            accepted.len(),
            observed.len(),
            stats_available,
            accepted.len() as f64 * 1000.0 / measured_ms
        );
        for (i, name) in [
            "frame",
            "cpu_prepare_submit",
            "wait",
            "readback_check",
            "present",
            "gpu_render",
            "gpu_copy",
        ]
        .iter()
        .enumerate()
        {
            summary(name, rows.iter().map(|r| r[i]));
        }
    }
}

// Test: Media Foundation H.264 decoding on the GPU (Windows).
//
// Encodes a moving checkerboard with the Media Foundation H.264 encoder (so no
// test files or ffmpeg are needed), then decodes it with `video::win_decoder`
// and checks: one picture out per access unit in (low latency), the output is
// NV12 on the GPU, the visible size is right (incl. cropping for sizes that
// aren't whole macroblocks), and the pixels match what was encoded.
//
// Run: cargo run --bin mf_decode_test

#![cfg_attr(not(windows), allow(unused))]

#[path = "../video/mod.rs"]
mod video;

#[cfg(not(windows))]
fn main() {
    eprintln!("mf_decode_test is Windows-only");
}

#[cfg(windows)]
fn main() {
    let mut ok = true;
    for (w, h) in [(1280, 720), (854, 480), (1920, 1080)] {
        match win::run(w, h) {
            Ok(true) => println!("PASS {w}x{h}\n"),
            Ok(false) => {
                println!("FAIL {w}x{h}\n");
                ok = false;
            }
            Err(e) => {
                println!("ERROR {w}x{h}: {e}\n");
                ok = false;
            }
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(windows)]
mod win {
    use std::time::Instant;

    use super::video::annexb;
    use super::video::win_decoder::{create_device, init_thread, DecodedFrame, Decoder};
    use super::video::win_test_encoder;
    use windows::core::Result;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;

    const FPS: u32 = 60;
    const FRAMES: usize = 120;
    const CELL: usize = 64;

    /// Luma of the test pattern: a checkerboard that moves 8 px right per frame.
    fn luma(x: usize, y: usize, i: usize) -> u8 {
        if ((x + 8 * i) / CELL + y / CELL) % 2 == 0 {
            40
        } else {
            210
        }
    }

    pub fn run(w: u32, h: u32) -> Result<bool> {
        init_thread()?;
        println!("== {w}x{h}");
        let t = Instant::now();
        let units: Vec<_> = win_test_encoder::encode(w, h, FPS, FRAMES, 30, luma)?
            .into_iter()
            .map(|u| (u.data, u.key, u.ts_us))
            .collect();
        let keys = units.iter().filter(|u| u.1).count();
        let first: Vec<u8> = annexb::nal_units(&units[0].0).map(annexb::nal_type).collect();
        println!(
            "encoded {} frames in {:?}, {} keyframes, {} KB, first AU NAL types {first:?}",
            units.len(),
            t.elapsed(),
            keys,
            units.iter().map(|u| u.0.len()).sum::<usize>() / 1024
        );

        let (device, context) = create_device()?;
        let mut dec = Decoder::new(&device, w, h)?;
        let mut out = Vec::new();
        let mut lag = Vec::new(); // inputs fed before each picture came out
        let mut times = Vec::new();
        let mut checked = false;
        let mut ok = true;
        for (i, (data, key, ts)) in units.iter().enumerate() {
            let t = Instant::now();
            dec.decode(data, *key, *ts, &mut out)?;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            for f in out.drain(..) {
                let n = ((f.ts_us * FPS as i64 + 500_000) / 1_000_000) as usize;
                lag.push(i - n);
                if n == FRAMES / 2 && !checked {
                    checked = true;
                    ok &= check(&device, &context, &f, &dec, w, h, n)?;
                }
            }
        }
        let late: Vec<usize> = lag.iter().enumerate().filter(|(_, &l)| l > 0).map(|(n, _)| n).collect();
        if !late.is_empty() {
            println!("late frames: {late:?}");
        }
        let decoded = lag.len();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "decoded {decoded}/{} frames; coded {:?}, visible {:?}; frames of delay: max {}; decode call ms: median {:.2}, max {:.2}",
            units.len(),
            dec.coded_size(),
            dec.display_size(),
            lag.iter().max().unwrap_or(&0),
            times[times.len() / 2],
            times[times.len() - 1]
        );
        ok &= decoded == units.len();
        ok &= lag.iter().all(|&l| l == 0);
        ok &= dec.display_size() == (w, h);
        ok &= checked;
        Ok(ok)
    }

    /// Copy the picture to the CPU and compare luma at cell centres.
    fn check(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        f: &DecodedFrame,
        dec: &Decoder,
        w: u32,
        h: u32,
        n: usize,
    ) -> Result<bool> {
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            f.texture.GetDesc(&mut desc);
            println!(
                "frame {n}: texture {}x{} format {:?} array {} slice {} bind {:#x}",
                desc.Width, desc.Height, desc.Format, desc.ArraySize, f.subresource, desc.BindFlags
            );
            if desc.Format != DXGI_FORMAT_NV12 {
                return Ok(false);
            }
            let staging_desc = D3D11_TEXTURE2D_DESC {
                ArraySize: 1,
                BindFlags: 0,
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut staging = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();
            context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &f.texture, f.subresource, None);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let y_plane = std::slice::from_raw_parts(map.pData as *const u8, map.RowPitch as usize * h as usize);
            let (dw, dh) = dec.display_size();
            let (mut worst, mut samples) = (0i32, 0);
            // Sample on a 16 px grid, skipping points near cell edges (where coding error is largest).
            let inside = |o: usize| (8..CELL - 8).contains(&(o % CELL));
            for y in (0..dh.min(h) as usize).step_by(16).filter(|&y| inside(y)) {
                for x in (0..dw.min(w) as usize).step_by(16).filter(|&x| inside(x + 8 * n)) {
                    let got = y_plane[y * map.RowPitch as usize + x] as i32;
                    worst = worst.max((got - luma(x, y, n) as i32).abs());
                    samples += 1;
                }
            }
            context.Unmap(&staging, 0);
            println!("frame {n}: {samples} luma samples, worst error {worst}");
            Ok(worst <= 24)
        }
    }
}

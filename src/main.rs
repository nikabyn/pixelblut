use std::cell::UnsafeCell;
use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

mod render;

mod encode;
use encode::PxFrameEncoder;

use crate::render::FrameRenderer;

// Wifi
// const ADDR: &str = "10.42.0.1:1234";
// const WIDTH: usize = 960;
// const HEIGHT: usize = 480;

// Ethernet direct
const ADDR: &str = "10.10.10.2:1234";
const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

// local
// const ADDR: &str = "0.0.0.0:1234";
// const WIDTH: usize = 1920;
// const HEIGHT: usize = 1080;

fn main() -> std::io::Result<()> {
    let mut clear = PixelflutStream::new(ADDR, 0, 0, 1920, 1080);
    for y in 0..1080 {
        clear.write_rgb_row(y as u16, &[[0; 3]; 1920]);
    }
    clear.flush().unwrap();
    drop(clear);

    PixelflutClient::new(ADDR, WIDTH as u16, HEIGHT as u16).run()
}

struct PixelflutClient {
    swapchain: SwapChain<(FrameRenderer, PxFrameEncoder)>,
    addr: Box<str>,
    width: u16,
    height: u16,
}

impl PixelflutClient {
    fn new(addr: &str, width: u16, height: u16) -> Self {
        Self {
            swapchain: SwapChain::new(
                (
                    FrameRenderer::new(width, height),
                    PxFrameEncoder::new(width, height),
                ),
                (
                    FrameRenderer::new(width, height),
                    PxFrameEncoder::new(width, height),
                ),
            ),
            addr: Box::from(addr),
            width,
            height,
        }
    }

    fn run(self) -> ! {
        let Self {
            swapchain,
            addr,
            width,
            ..
        } = self;
        let swapchain = Arc::new(swapchain);
        let barrier = Arc::new(Barrier::new(2));
        let start = Instant::now();

        std::thread::spawn({
            let swapchain = swapchain.clone();
            let barrier = barrier.clone();
            move || {
                loop {
                    let measure_start = Instant::now();

                    let (renderer, encoder) = unsafe { swapchain.front().as_mut_unchecked() };
                    let t = start.elapsed().as_secs_f32();
                    let pixels = renderer.render(t);
                    for (y, row) in pixels.chunks_exact(width as usize).enumerate() {
                        unsafe { encoder.update_row(y as u16, row) };
                    }

                    let delta_t = measure_start.elapsed();
                    let fps = Duration::from_secs(1).div_duration_f32(delta_t);
                    // println!("render average: {delta_t:?} per frame, {fps:?}fps");

                    barrier.wait();
                }
            }
        });

        let mut tcp = TcpStream::connect(&*addr).unwrap();
        tcp.set_nodelay(true).unwrap();
        tcp.set_nonblocking(false).unwrap();

        let mut i: u32 = 0;
        let mut measure_start = Instant::now();
        loop {
            barrier.wait();
            let (_, encoder) = unsafe { swapchain.back().as_ref_unchecked() };
            tcp.write_all(encoder.encoded()).unwrap();
            swapchain.swap();

            if measure_start.elapsed() >= Duration::from_secs(1) {
                let delta_t = measure_start.elapsed() / i;
                let fps = Duration::from_secs(1).div_duration_f32(delta_t);
                println!("send average: {delta_t:?} per frame, {fps:?}fps");
                measure_start = Instant::now();
                i = 0;
            }
            i = i.wrapping_add(1);
        }
    }
}

struct SwapChain<T> {
    chain: [UnsafeCell<T>; 2],
    swapped: AtomicBool,
}

unsafe impl<A> Sync for SwapChain<A> {}

impl<T> SwapChain<T> {
    fn new(a: T, b: T) -> Self {
        Self {
            chain: [UnsafeCell::new(a), UnsafeCell::new(b)],
            swapped: AtomicBool::new(false),
        }
    }

    fn swap(&self) {
        self.swapped.fetch_xor(true, Ordering::AcqRel);
    }

    unsafe fn front(&self) -> *mut T {
        let i = if self.swapped.load(Ordering::Acquire) {
            0
        } else {
            1
        };
        self.chain[i].get()
    }

    unsafe fn back(&self) -> *const T {
        let i = if self.swapped.load(Ordering::Acquire) {
            1
        } else {
            0
        };
        self.chain[i].get()
    }
}

struct PixelflutStream {
    inner: TcpStream,
    encoder: PxFrameEncoder,
    offset_x: u16,
    offset_y: u16,
}

impl PixelflutStream {
    fn new(addr: &str, offset_x: u16, offset_y: u16, width: u16, height: u16) -> Self {
        let inner = TcpStream::connect(addr).unwrap();
        inner.set_nodelay(true).unwrap();
        inner.set_nonblocking(false).unwrap();

        let mut stream = Self {
            inner,
            encoder: PxFrameEncoder::new(width, height),
            offset_x,
            offset_y,
        };
        stream.flush_offset().unwrap();
        stream
    }

    fn write_rgb_row(&mut self, y: u16, row: &[[u8; 3]]) {
        unsafe { self.encoder.update_row(y, row) };
    }

    fn flush_offset(&mut self) -> io::Result<()> {
        self.inner
            .write_all(format!("OFFSET {} {}\n", self.offset_x, self.offset_y).as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        // eprintln!("{}", String::from_utf8_lossy(&self.cache));
        self.inner.write_all(&self.encoder.encoded())
    }
}

use std::cell::UnsafeCell;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

mod encode;
use encode::PxFrameEncoder;

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

fn render(pixels: &mut Pixels, t_secs: f32) {
    let t = (t_secs % 3.0) / 3.0;

    let width = pixels.width();
    let height = pixels.height();
    let half_width = width / 2;
    let half_height = height / 2;

    let box_center_x = (half_width as i32 + (half_width as f32 * (t * 2.0 - 1.0)) as i32) as usize;
    let box_center_y =
        (half_height as i32 - (half_height as f32 * (t * 2.0 - 1.0)) as i32) as usize;
    let box_width = height as usize / 10;
    let box_height = box_width;
    let box_x = (box_center_x - box_width / 2)..(box_center_x + box_width / 2);
    let box_y = (box_center_y - box_height / 2)..(box_center_y + box_height / 2);

    for y in 0..(height as usize) {
        let base = unsafe { pixels.as_slice_mut().as_mut_ptr().add(y * width as usize) };

        let v = (y as f32 / height as f32 * 255.0) as u8;
        for x in 0..(width as usize) {
            let [r, g, b] = unsafe { base.add(x).as_mut_unchecked() };
            let u = (x as f32 / width as f32 * 255.0) as u8;
            *r = (u as f32 * t) as u8;
            *g = (v as f32 * t) as u8;
            *b = 0;

            if box_x.contains(&x) && box_y.contains(&y) {
                *r = 0xff;
                *g = 0xff;
                *b = 0xff;
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut tcp: TcpStream = TcpStream::connect(&*ADDR).unwrap();
    tcp.set_nodelay(true).unwrap();
    tcp.set_nonblocking(false).unwrap();

    let mut encoder = PxFrameEncoder::new(1920, 1080);
    for y in 0..1080 {
        unsafe { encoder.update_row(y as u16, &[[0; 3]; 1920]) };
    }
    tcp.write_all(encoder.encoded()).unwrap();
    drop(encoder);

    let (width, height) = (WIDTH as u16, HEIGHT as u16);
    let swapchain: Arc<SwapChain<(Pixels, PxFrameEncoder)>> = Arc::new(SwapChain::new(
        (
            Pixels::new(width, height),
            PxFrameEncoder::new(width, height),
        ),
        (
            Pixels::new(width, height),
            PxFrameEncoder::new(width, height),
        ),
    ));
    let barrier = Arc::new(Barrier::new(2));
    let start = Instant::now();

    std::thread::spawn({
        let swapchain = swapchain.clone();
        let barrier = barrier.clone();
        move || {
            loop {
                let measure_start = Instant::now();

                let (pixels, encoder) = unsafe { swapchain.front().as_mut_unchecked() };
                let t_secs = start.elapsed().as_secs_f32();
                render(pixels, t_secs);
                for (y, row) in pixels.as_slice().chunks_exact(width as usize).enumerate() {
                    unsafe { encoder.update_row(y as u16, row) };
                }

                let delta_t = measure_start.elapsed();
                let fps = Duration::from_secs(1).div_duration_f32(delta_t);
                println!("render average: {delta_t:?} per frame, {fps:?}fps");

                barrier.wait();
            }
        }
    });

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

pub struct Pixels {
    pixels: Box<[[u8; 3]]>,
    width: u16,
    height: u16,
}

impl Pixels {
    pub fn new(width: u16, height: u16) -> Self {
        let pixels_uninit = Box::new_uninit_slice(width as usize * height as usize);
        Self {
            // SAFETY Pixels always have a valid state.
            //        The only way to access them is through render,
            //        which overrides all of them.
            pixels: unsafe { pixels_uninit.assume_init() },
            width,
            height,
        }
    }

    pub fn as_slice(&self) -> &[[u8; 3]] {
        &self.pixels
    }

    pub fn as_slice_mut(&mut self) -> &mut [[u8; 3]] {
        &mut self.pixels
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}

use std::cell::UnsafeCell;
use std::io::Write;
use std::mem::MaybeUninit;
use std::net::TcpStream;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

mod encode;
use encode::{BasicTextEncoder, InterlacedTextEncoder, TextEncoder};

// Wifi
// const ADDR: &str = "10.42.0.1:1234";
// const WIDTH: usize = 960;
// const HEIGHT: usize = 540;

// Ethernet direct
const ADDR: &str = "10.10.10.2:1234";
const WIDTH: usize = 960;
const HEIGHT: usize = 540;

// local
// const ADDR: &str = "0.0.0.0:1234";
// const WIDTH: usize = 1920;
// const HEIGHT: usize = 1080;

fn render(pixels: &mut Pixels, t_secs: f32) {
    let t = (t_secs % 1.0) / 1.0;

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
        let base = &mut pixels.as_slice_mut()[y * width as usize..];

        let v = (y as f32 / height as f32 * 255.0) as u8;
        for x in 0..(width as usize) {
            let [r, g, b] = &mut base[x];
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

    let mut encoder = BasicTextEncoder::new(1920, 1080);
    let encoded = encoder.encode(&Pixels::new(1920, 1080, [0; 3]));
    tcp.write_all(encoded).unwrap();
    drop(encoder);

    let (width, height) = (WIDTH as u16, HEIGHT as u16);
    let swapchain = SwapChain::new(
        (
            Pixels::new(width, height, [0; 3]),
            InterlacedTextEncoder::new(width, height, 0),
        ),
        (
            Pixels::new(width, height, [0; 3]),
            InterlacedTextEncoder::new(width, height, 1),
        ),
    );

    let start = Instant::now();
    swapchain.run(
        move |(pixels, encoder)| {
            let t_secs = start.elapsed().as_secs_f32();
            render(pixels, t_secs);
            encoder.encode(pixels);
        },
        |(_, encoder)| {
            tcp.write_all(encoder.cache()).unwrap();
        },
    )
}

struct SwapChain<T> {
    chain: [UnsafeCell<T>; 2],
}

impl<T: 'static> SwapChain<T> {
    fn new(a: T, b: T) -> Self {
        Self {
            chain: [UnsafeCell::new(a), UnsafeCell::new(b)],
        }
    }

    fn run(
        self,
        mut write: impl FnMut(&mut T) -> () + std::marker::Send + 'static,
        mut read: impl FnMut(&T) -> () + std::marker::Send,
    ) -> ! {
        let barrier = Arc::new(Barrier::new(2));
        let front = Arc::new(AtomicPtr::new(self.chain[0].get()));
        let mut back = self.chain[1].get();

        std::thread::spawn({
            let barrier = barrier.clone();
            let front = front.clone();
            move || {
                let mut measure_start = Instant::now();
                loop {
                    let render_start = Instant::now();

                    let current_front = front.load(Ordering::Acquire);
                    write(unsafe { current_front.as_mut_unchecked() });

                    barrier.wait();

                    if measure_start.elapsed() >= Duration::from_secs(1) {
                        let delta_t = render_start.elapsed();
                        let fps = Duration::from_secs(1).div_duration_f32(delta_t);
                        println!("render: {delta_t:?} per frame, {fps:?}fps");
                        measure_start = Instant::now();
                    }

                    barrier.wait();
                }
            }
        });

        let mut i: u32 = 0;
        let mut measure_start = Instant::now();
        loop {
            barrier.wait();

            let completed = front.swap(back, Ordering::AcqRel);
            back = completed;
            read(unsafe { back.as_ref_unchecked() });

            if measure_start.elapsed() >= Duration::from_secs(1) {
                let delta_t = measure_start.elapsed() / i;
                let fps = Duration::from_secs(1).div_duration_f32(delta_t);
                println!("send: {delta_t:?} per frame, {fps:?}fps");
                measure_start = Instant::now();
                i = 0;
            }
            i = i.wrapping_add(1);

            barrier.wait();
        }
    }
}

pub struct Pixels {
    pixels: Box<[[u8; 3]]>,
    width: u16,
    height: u16,
}

impl Pixels {
    pub fn new(width: u16, height: u16, fill: [u8; 3]) -> Self {
        let mut pixels_uninit = Box::new_uninit_slice(width as usize * height as usize);
        pixels_uninit.fill(MaybeUninit::new(fill));
        Self {
            // SAFETY Pixels have been initialized
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

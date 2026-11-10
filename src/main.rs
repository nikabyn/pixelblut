use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use std::thread::{self, park_timeout};
use std::time::{Duration, Instant};

mod render;
use render::*;

mod encode;
use encode::{PIXEL_SIZE, PxFrameEncoder};

// Wifi
// const ADDR: &str = "10.42.0.1:1234";
// const WIDTH: usize = 960;
// const HEIGHT: usize = 480;
// const NUM_SEND_THREADS: usize = 1;
// const NUM_GEN_THREADS: usize = 4;
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = None;

// Ethernet direct
const ADDR: &str = "10.10.10.2:1234";
const WIDTH: usize = 1920 / 2;
const HEIGHT: usize = 1080 / 2;
const NUM_SEND_THREADS: usize = 1;
const NUM_GEN_THREADS: usize = 8;
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const CHUNK_FLUSH_TIMEOUT: Option<Duration> = None;

// local
// const ADDR: &str = "0.0.0.0:1234";
// const WIDTH: usize = 1920;
// const HEIGHT: usize = 1080;
// const NUM_SEND_THREADS: usize = 8;
// const NUM_GEN_THREADS: usize = 8;
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = None;

fn main() -> std::io::Result<()> {
    static mut PIXEL_BUFFER: [u8; PIXEL_SIZE * WIDTH * HEIGHT] = [0; _];
    let pixels = (&raw mut PIXEL_BUFFER).cast::<[u8; 3]>();

    let mut clear = PixelflutStream::new(ADDR, 0, 0, 1920, 1080);
    for y in 0..1080 {
        clear.write_rgb_row(y as u16, &[[0; 3]; 1920]);
    }
    clear.flush(None).unwrap();
    drop(clear);

    let aspect_ratio = WIDTH as f64 / HEIGHT as f64;

    let chunks_x = ((NUM_SEND_THREADS as f64 * aspect_ratio).sqrt()).round() as usize;
    let chunks_y = NUM_SEND_THREADS.div_ceil(chunks_x);

    let send_barrier: Arc<Barrier> = Arc::new(Barrier::new(0));
    for chunk in chunks(pixels, chunks_x, chunks_y) {
        let send_barrier = send_barrier.clone();
        thread::spawn(|| {
            send_pixels_worker(chunk, send_barrier);
        });
    }

    run_render_loop(pixels, WIDTH, HEIGHT);
}

// #[inline(never)]
fn send_pixels_worker(mut chunk: Chunk, barrier: Arc<Barrier>) {
    // eprintln!("worker {} connected", chunk.index);

    let min_x: u16 = chunk.offset_x as u16;
    let min_y: u16 = chunk.offset_y as u16;
    let max_x: u16 = min_x + chunk.width as u16;
    let max_y: u16 = min_y + chunk.height as u16;

    let mut stream = PixelflutStream::new(ADDR, min_x, min_y, max_x - min_x, max_y - min_y);

    let mut start = Instant::now();
    let mut i = 0;
    loop {
        barrier.wait();

        for (y, row) in chunk.rows().enumerate() {
            stream.write_rgb_row(y as u16, row);
        }
        if let Err(err) = stream.flush(CHUNK_FLUSH_TIMEOUT)
            && err.kind() == io::ErrorKind::TimedOut
        {
            eprintln!("Flush timed out!");
            stream.reconnect_tcp().unwrap();
        }

        if start.elapsed() > Duration::from_millis(1000) {
            let delta_t = start.elapsed() / (i + 1);
            let send_fps = Duration::from_secs(1).as_nanos() / delta_t.as_nanos();
            eprintln!("send time = {delta_t:?}");
            eprintln!("send fps = {send_fps:?}");
            i = 0;
            start = Instant::now();
        }
        i += 1;
    }
}

fn chunks(pixels: *mut [u8; 3], chunks_x: usize, chunks_y: usize) -> impl Iterator<Item = Chunk> {
    (0..chunks_x)
        .flat_map(move |x| (0..chunks_y).map(move |y| (x, y)))
        .map(move |(index_x, index_y)| {
            let offset_x = index_x * WIDTH / chunks_x;
            let offset_y = index_y * HEIGHT / chunks_y;

            let end_x = (index_x + 1) * WIDTH / chunks_x;
            let end_y = (index_y + 1) * HEIGHT / chunks_y;

            let width = end_x - offset_x;
            let height = end_y - offset_y;

            let offset = offset_y * WIDTH + offset_x;

            Chunk {
                pixels: unsafe { pixels.add(offset) },
                offset_x,
                offset_y,
                width,
                height,
            }
        })
}

struct Chunk {
    pixels: *mut [u8; 3],
    offset_x: usize,
    offset_y: usize,
    width: usize,
    height: usize,
}

impl Chunk {
    fn rows(&mut self) -> impl Iterator<Item = &mut [[u8; 3]]> + '_ {
        let pixels = self.pixels;
        let width = self.width;
        let height = self.height;

        (0..height)
            .map(move |y| unsafe { std::slice::from_raw_parts_mut(pixels.add(y * WIDTH), width) })
    }
}

// Each chunk owns a distinct rectangular region.
// Mutable slices are only created for individual rows.
unsafe impl Send for Chunk {}

struct PixelflutStream {
    inner: TcpStream,
    encoder: PxFrameEncoder,
    offset_x: u16,
    offset_y: u16,
    width: u16,
    height: u16,
}

impl PixelflutStream {
    fn new(addr: &str, offset_x: u16, offset_y: u16, width: u16, height: u16) -> Self {
        let inner = TcpStream::connect(addr).unwrap();
        inner.set_nonblocking(true).unwrap();

        let mut stream = Self {
            inner,
            encoder: PxFrameEncoder::new(width, height),
            offset_x,
            offset_y,
            width,
            height,
        };
        stream.flush_offset().unwrap();
        stream
    }

    fn reconnect_tcp(&mut self) -> io::Result<()> {
        let new_tcp = TcpStream::connect(ADDR)?;
        new_tcp.set_nonblocking(true)?;
        _ = std::mem::replace(&mut self.inner, new_tcp);
        self.flush_offset()
    }

    fn write_rgb_row(&mut self, y: u16, row: &[[u8; 3]]) {
        unsafe { self.encoder.update_row(y, row) };
    }

    fn flush_offset(&mut self) -> io::Result<()> {
        self.inner
            .write_all(format!("OFFSET {} {}\n", self.offset_x, self.offset_y).as_bytes())
    }

    #[inline(never)]
    fn flush(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let start = Instant::now();

        let mut buf = &*self.encoder.encoded();
        while !buf.is_empty() {
            if let Some(timeout) = timeout
                && start.elapsed() > timeout
            {
                _ = self.inner.write_all(b"\n");
                return Err(io::Error::new(io::ErrorKind::TimedOut, ""));
            }

            match self.inner.write(buf) {
                Ok(0) => break,
                Ok(n) => buf = &buf[n..],
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    park_timeout(Duration::from_micros(10));
                }
                Err(e) => eprintln!("{e}"),
            }
        }

        Ok(())

        // eprintln!("{}", String::from_utf8_lossy(&self.cache));
        // self.inner.write_all(&self.cache)
    }
}

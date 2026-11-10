use std::io::{self, Write};
use std::net::TcpStream;
use std::num::NonZeroU32;
use std::sync::{Arc, Barrier};
use std::thread::{self, park_timeout};
use std::time::{Duration, Instant};

// Wifi
// const ADDR: &str = "10.42.0.1:1234";
// const WIDTH: usize = 960;
// const HEIGHT: usize = 480;
// const NUM_SEND_THREADS: usize = 480 / 8;
// const NUM_GEN_THREADS: usize = 4;
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = Some(Duration::from_millis(200));

// Ethernet direct
const ADDR: &str = "10.10.10.2:1234";
const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
const NUM_SEND_THREADS: usize = 1;
const NUM_GEN_THREADS: usize = 8;
const CHUNK_FLUSH_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = None;

// local
// const ADDR: &str = "0.0.0.0:1234";
// const WIDTH: usize = 1920;
// const HEIGHT: usize = 1080;
// const NUM_SEND_THREADS: usize = 8;
// const NUM_GEN_THREADS: usize = 8;
// const CHUNK_FLUSH_TIMEOUT: Option<Duration> = None;

const PIXEL_SIZE: usize = 3;
static mut PIXEL_BUFFER: [u8; PIXEL_SIZE * WIDTH * HEIGHT] = [0; _];

fn main() -> std::io::Result<()> {
    let mut clear = PixelflutStream::new(ADDR, 0, 0, 1920, 1080);
    for y in 0..1080 {
        clear.write_rgb_row(y as u16, &[0_u8; 1920 * PIXEL_SIZE]);
    }
    clear.flush(None).unwrap();
    drop(clear);

    let aspect_ratio = WIDTH as f64 / HEIGHT as f64;

    let chunks_x = ((NUM_SEND_THREADS as f64 * aspect_ratio).sqrt()).round() as usize;
    let chunks_y = NUM_SEND_THREADS.div_ceil(chunks_x);

    let send_barrier: Arc<Barrier> = Arc::new(Barrier::new(0));
    for chunk in chunks(chunks_x, chunks_y) {
        let send_barrier = send_barrier.clone();
        thread::spawn(|| {
            send_pixels_worker(chunk, send_barrier);
        });
    }

    generate_pixels();
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
            // eprintln!("send fps = {send_fps:?}");
            i = 0;
            start = Instant::now();
        }
        i += 1;
    }
}

fn generate_pixels() -> ! {
    let base = (&raw mut PIXEL_BUFFER).cast::<[u8; 3]>();

    let max_fps = NonZeroU32::new(60).unwrap();
    let clock = chron::Clock::new(NonZeroU32::new(120).unwrap()).max_frame_rate(max_fps);

    let start = Instant::now();
    for tick in clock {
        if matches!(tick, chron::Tick::Update) {
            continue;
        }

        let elapsed = start.elapsed().as_secs_f32();
        let t = (elapsed % 1.0) / 1.0;

        let box_center_x =
            (WIDTH as i32 / 2 + ((WIDTH / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_center_y =
            (HEIGHT as i32 / 2 - ((HEIGHT / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_width = 40;
        let box_height = 40;
        let box_x = (box_center_x - box_width / 2)..(box_center_x + box_width / 2);
        let box_y = (box_center_y - box_height / 2)..(box_center_y + box_height / 2);

        for y in 0..HEIGHT {
            let base = unsafe { base.add(y * WIDTH) };

            let v = (y as f32 / HEIGHT as f32 * 255.0) as u8;
            for x in 0..WIDTH {
                let [r, g, b] = unsafe { base.add(x).as_mut_unchecked() };
                let u = (x as f32 / WIDTH as f32 * 255.0) as u8;
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

    unreachable!()
}

fn chunks(chunks_x: usize, chunks_y: usize) -> impl Iterator<Item = Chunk> {
    let base = (&raw mut PIXEL_BUFFER).cast::<u8>();

    (0..chunks_x)
        .flat_map(move |x| (0..chunks_y).map(move |y| (x, y)))
        .map(move |(index_x, index_y)| {
            let offset_x = index_x * WIDTH / chunks_x;
            let offset_y = index_y * HEIGHT / chunks_y;

            let end_x = (index_x + 1) * WIDTH / chunks_x;
            let end_y = (index_y + 1) * HEIGHT / chunks_y;

            let width = end_x - offset_x;
            let height = end_y - offset_y;

            let offset = offset_y * WIDTH * PIXEL_SIZE + offset_x * PIXEL_SIZE;

            Chunk {
                pixels: unsafe { base.add(offset) },
                offset_x,
                offset_y,
                width,
                height,
            }
        })
}

struct Chunk {
    pixels: *mut u8,
    offset_x: usize,
    offset_y: usize,
    width: usize,
    height: usize,
}

impl Chunk {
    fn rows(&mut self) -> impl Iterator<Item = &mut [u8]> + '_ {
        let pixels = self.pixels;
        let width = self.width;
        let height = self.height;

        (0..height).map(move |y| unsafe {
            std::slice::from_raw_parts_mut(pixels.add(y * WIDTH * PIXEL_SIZE), width * PIXEL_SIZE)
        })
    }
}

// Each chunk owns a distinct rectangular region.
// Mutable slices are only created for individual rows.
unsafe impl Send for Chunk {}

struct PixelflutStream {
    inner: TcpStream,
    cache: Box<[u8]>,
    rgb_offsets: Box<[usize]>,
    offset_x: u16,
    offset_y: u16,
    width: u16,
    height: u16,
}

impl PixelflutStream {
    fn new(addr: &str, offset_x: u16, offset_y: u16, width: u16, height: u16) -> Self {
        let inner = TcpStream::connect(addr).unwrap();
        inner.set_nonblocking(true).unwrap();

        let mut cache = Vec::with_capacity(
            width as usize * height as usize * size_of_val(b"PX xxxxx yyyyy ffffff\n"),
        );
        let mut rgb_offsets = Vec::with_capacity(width as usize * height as usize);

        for y in 0..height {
            for x in 0..width {
                cache.extend_from_slice(b"PX ");

                let mut x_buffer = [0; 5];
                let x_len = encode_u16(x, &mut x_buffer);
                cache.extend_from_slice(&x_buffer[5 - x_len..]);

                cache.push(b' ');

                let mut y_buffer = [0; 5];
                let y_len = encode_u16(y, &mut y_buffer);
                cache.extend_from_slice(&y_buffer[5 - y_len..]);

                cache.push(b' ');

                // Skip rgb hex section
                rgb_offsets.push(cache.len());
                cache.extend_from_slice(b"ffffff");

                cache.push(b'\n');
            }
        }

        cache.shrink_to_fit();

        dbg!(cache.len());

        let mut stream = Self {
            inner,
            cache: cache.into_boxed_slice(),
            rgb_offsets: rgb_offsets.into_boxed_slice(),
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

    fn pixel_key(x: u16, y: u16, width: u16) -> usize {
        y as usize * width as usize + x as usize
    }

    #[inline(never)]
    fn write_rgb_row(&mut self, y: u16, row: &[u8]) {
        static HEX_U8: [u16; 256] = {
            let mut table = [0; 256];
            let chars = b"0123456789abcdef";

            let mut i = 0;
            while i < 256 {
                table[i] = u16::from_le_bytes([chars[i / 16], chars[i % 16]]);
                i += 1;
            }

            table
        };

        let start_key = Self::pixel_key(0, y, self.width);

        let mut offset = unsafe { *self.rgb_offsets.get_unchecked(start_key) };
        let mut stride = size_of_val(b"PX   ffffff\n").wrapping_add(digits_u16(y)) + 1;
        let mut next_boundary = 9;

        for (x, [r, g, b]) in unsafe { row.as_chunks_unchecked::<3>() }.iter().enumerate() {
            let inc_stride = x == next_boundary;
            next_boundary = if inc_stride {
                next_boundary * 10 + 9
            } else {
                next_boundary
            };
            stride = stride.wrapping_add(1 * inc_stride as usize);

            let r = HEX_U8[*r as usize];
            let g = HEX_U8[*g as usize];
            let b = HEX_U8[*b as usize];

            unsafe {
                let dst = self.cache.as_mut_ptr().add(offset).cast::<u16>();

                dst.write_unaligned(r);
                dst.add(1).write_unaligned(g);
                dst.add(2).write_unaligned(b);
            }

            offset = offset.wrapping_add(stride);
        }
    }

    fn flush_offset(&mut self) -> io::Result<()> {
        self.inner
            .write_all(format!("OFFSET {} {}\n", self.offset_x, self.offset_y).as_bytes())
    }

    #[inline(never)]
    fn flush(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let start = Instant::now();

        let mut buf = &*self.cache;
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

fn encode_u16(value: u16, buffer: &mut [u8; 5]) -> usize {
    buffer[0] = b'0' + (value / 10_000 % 10) as u8;
    buffer[1] = b'0' + (value / 1_000 % 10) as u8;
    buffer[2] = b'0' + (value / 100 % 10) as u8;
    buffer[3] = b'0' + (value / 10 % 10) as u8;
    buffer[4] = b'0' + (value / 1 % 10) as u8;

    digits_u16(value)
}

fn digits_u16(value: u16) -> usize {
    match value {
        ..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        1_000..=9_999 => 4,
        10_000.. => 5,
    }
}

fn encode_rgb_hex(rgb: [u8; 3], buffer: &mut [u8; 6]) {
    let chars = b"0123456789abcdef";

    let [r, g, b] = rgb;
    buffer[0] = chars[(r / 16) as usize];
    buffer[1] = chars[(r % 16) as usize];
    buffer[2] = chars[(g / 16) as usize];
    buffer[3] = chars[(g % 16) as usize];
    buffer[4] = chars[(b / 16) as usize];
    buffer[5] = chars[(b % 16) as usize];
}

#[test]
fn benchmark_encode() {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    static mut PIXEL_BUFFER: [u8; PIXEL_SIZE * WIDTH * HEIGHT] = [0; _];

    for (i, byte) in unsafe { PIXEL_BUFFER }.iter_mut().enumerate() {
        *byte = match i {
            0..12 => 0x0f,
            15..52 => 0x12,
            60..80 => 0xff,
            100..200 => i as u8,
            _ => 0x00,
        };
    }

    let mut chunk = Chunk {
        pixels: (&raw mut PIXEL_BUFFER).cast::<u8>(),
        offset_x: 0,
        offset_y: 0,
        width: 1920,
        height: 1080,
    };

    let mut stream = PixelflutStream::new(ADDR, 0, 0, 1920, 1080);

    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        for (y, row) in chunk.rows().enumerate() {
            stream.write_rgb_row(y as u16, row);
        }
    }
    let average_encode_t = start.elapsed() / iterations;
    let average_encode_fps = Duration::from_secs(1).as_nanos() / average_encode_t.as_nanos();
    println!("average encode time = {average_encode_t:?}");
    println!("average encode fps = {average_encode_fps:?}");
}

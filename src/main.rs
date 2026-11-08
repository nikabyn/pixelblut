#![allow(static_mut_refs)]

use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use std::thread::{self, park};
use std::time::Instant;

// Wifi
// const ADDR: &str = "10.42.0.1:1234";
// const WIDTH: usize = 960;
// const HEIGHT: usize = 480;
// const NUM_SEND_THREADS: usize = 480 / 8;
// const NUM_GEN_THREADS: usize = 4;

// Ethernet direct
const ADDR: &str = "10.10.10.2:1234";
const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
const NUM_SEND_THREADS: usize = 135;
const NUM_GEN_THREADS: usize = 4;

// local
// const ADDR: &str = "0.0.0.0:1234";
// const WIDTH: usize = 1920;
// const HEIGHT: usize = 1080;
// const NUM_SEND_THREADS: usize = 8;
// const NUM_GEN_THREADS: usize = 8;

const PIXEL_SIZE: usize = 3;
static mut PIXEL_BUFFER: [u8; PIXEL_SIZE * WIDTH * HEIGHT] = [0; _];

fn main() -> std::io::Result<()> {
    let stream = TcpStream::connect(ADDR).unwrap();
    let mut clear = PixelflutStream::new(stream, 1920, 1080);
    for y in 0..1080 {
        clear.write_rgb_row(y as u16, &[0_u8; 1920 * PIXEL_SIZE]);
    }
    clear.flush().unwrap();
    drop(clear);

    let send_barrier: Arc<Barrier> = Arc::new(Barrier::new(NUM_SEND_THREADS));
    for chunk in chunks(NUM_SEND_THREADS) {
        let send_barrier = send_barrier.clone();
        thread::spawn(|| {
            send_pixels_worker(chunk, send_barrier);
        });
    }

    let generate_barrier: Arc<Barrier> = Arc::new(Barrier::new(NUM_GEN_THREADS));
    for chunk in chunks(NUM_GEN_THREADS) {
        let generate_barrier = generate_barrier.clone();
        thread::spawn(|| {
            generate_pixels_worker(chunk, generate_barrier);
        });
    }

    loop {
        park();
    }
}

#[inline(never)]
fn send_pixels_worker(mut chunk: Chunk<'_>, barrier: Arc<Barrier>) {
    // eprintln!("worker {} connected", chunk.index);

    let stream = TcpStream::connect(ADDR).unwrap();
    let min_x: u16 = 0;
    let min_y: u16 = chunk.rows_start as u16;
    let max_x: u16 = WIDTH as u16;
    let max_y: u16 = chunk.rows_end as u16;

    let mut stream = PixelflutStream::new(stream, max_x - min_x, max_y - min_y);
    stream.offset(min_x, min_y).unwrap();

    loop {
        barrier.wait();

        for (y, row) in chunk.rows().enumerate() {
            stream.write_rgb_row(y as u16, row);
        }
        stream.flush().unwrap();
    }
}

#[inline(never)]
fn generate_pixels_worker(mut chunk: Chunk<'_>, barrier: Arc<Barrier>) {
    let start = Instant::now();

    loop {
        barrier.wait();

        let now = Instant::now();
        let elapsed = now.duration_since(start).as_secs_f32();
        let t = (elapsed % 1.0) / 1.0;

        let rows_start = chunk.rows_start;
        let box_center_x =
            (WIDTH as i32 / 2 + ((WIDTH / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_center_y =
            (HEIGHT as i32 / 2 - ((HEIGHT / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_width = 20;
        let box_height = 20;
        let box_x = (box_center_x - box_width / 2)..(box_center_x + box_width / 2);
        let box_y = (box_center_y - box_height / 2)..(box_center_y + box_height / 2);

        for (row_index, row) in chunk.rows().enumerate() {
            let y = row_index + rows_start;
            let v = (y as f32 / HEIGHT as f32 * 255.0) as u8;
            for (x, rgb) in unsafe { row.as_chunks_unchecked_mut::<3>() }
                .iter_mut()
                .enumerate()
            {
                let [r, g, b] = rgb;
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
}

fn chunks(num_chunks: usize) -> impl Iterator<Item = Chunk<'static>> {
    let num_rows = HEIGHT / num_chunks;
    let row_size = WIDTH * PIXEL_SIZE;
    let chunk_size = row_size * num_rows;

    (0..num_chunks).map(move |index| {
        let start_row = index * num_rows;
        let start_byte = start_row * row_size;
        Chunk {
            index,
            pixels: unsafe { &mut PIXEL_BUFFER[start_byte..start_byte + chunk_size] },
            rows_start: start_row,
            rows_end: start_row + num_rows,
        }
    })
}

struct Chunk<'a> {
    index: usize,
    pixels: &'a mut [u8],
    rows_start: usize,
    rows_end: usize,
}

impl<'a> Chunk<'a> {
    fn rows(&mut self) -> impl Iterator<Item = &mut [u8]> {
        self.pixels.chunks_exact_mut(WIDTH * PIXEL_SIZE)
    }
}

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
    #[inline(never)]
    fn new(inner: TcpStream, width: u16, height: u16) -> Self {
        // inner.set_nodelay(true).unwrap();

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
        Self {
            inner,
            cache: cache.into_boxed_slice(),
            rgb_offsets: rgb_offsets.into_boxed_slice(),
            offset_x: 0,
            offset_y: 0,
            width,
            height,
        }
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

    fn offset(&mut self, offset_x: u16, offset_y: u16) -> io::Result<()> {
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        self.inner
            .write_all(format!("OFFSET {offset_x} {offset_y}\n").as_bytes())
    }

    #[inline(never)]
    fn flush(&mut self) -> io::Result<()> {
        // eprintln!("{}", String::from_utf8_lossy(&self.cache));
        self.inner.write_all(&self.cache)
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

#[inline]
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

#![allow(static_mut_refs)]

use std::io::{self, Write};
use std::net::TcpStream;
use std::thread::{self, sleep};
use std::time::Duration;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const PIXEL_SIZE: usize = 3;
const PIXEL_BUFFER_SIZE: usize = PIXEL_SIZE * WIDTH * HEIGHT;
static mut PIXEL_BUFFER: [u8; PIXEL_BUFFER_SIZE] = [0; _];

const NUM_THREADS: usize = 24;

fn main() -> std::io::Result<()> {
    let colors = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [255, 0, 255],
        [0, 255, 255],
    ];

    let stream = TcpStream::connect("10.42.0.1:1234").unwrap();
    let mut clear = PixelflutStream::new(stream, WIDTH as u16, HEIGHT as u16);
    for x in 0..WIDTH {
        for y in 0..HEIGHT {
            clear.write_px_rgb(x as u16, y as u16, [0, 0, 0]);
        }
        clear.flush().unwrap();
    }
    drop(clear);

    for mut chunk in chunks(NUM_THREADS) {
        thread::spawn(move || {
            let stream = TcpStream::connect("10.42.0.1:1234").unwrap();
            let min_x: u16 = 0;
            let min_y: u16 = chunk.rows_start as u16;
            let max_x: u16 = WIDTH as u16;
            let max_y: u16 = chunk.rows_end as u16;

            let mut stream = PixelflutStream::new(stream, max_x - min_x, max_y - min_y);
            stream.offset(min_x, min_y).unwrap();

            let color = colors[chunk.index % colors.len()];

            for i in 0.. {
                for (row_index, row) in chunk.rows().enumerate() {
                    let y = row_index;
                    for (x, rgb) in unsafe { row.as_chunks_unchecked::<PIXEL_SIZE>() }
                        .iter()
                        .enumerate()
                    {
                        stream.write_px_rgb(
                            x as u16,
                            y as u16,
                            if i % 2 == 0 { color } else { *rgb },
                            // color,
                            // *rgb,
                        );
                    }
                }
                stream.flush().unwrap();
            }
        });
    }

    sleep(Duration::from_hours(1));
    let mut i: usize = 0;
    loop {
        i = i.wrapping_add(1);
        let t = (i % 1000) as f32 / 1000.0;

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let u = (x as f32 / WIDTH as f32 * 255.0) as u8;
                let v = (y as f32 / HEIGHT as f32 * 255.0) as u8;

                let r = (u as f32 * t) as u8;
                let g = (v as f32 * t) as u8;

                set_pixel(unsafe { &mut PIXEL_BUFFER }, x, y, [r, g, 0]);
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
    buffer: Box<[u8]>,
    buffer_len: usize,
    px_rgb_cache: Box<[PxRgbTemplate]>,
    offset_x: u16,
    offset_y: u16,
    width: u16,
    height: u16,
}

impl PixelflutStream {
    #[inline(never)]
    fn new(inner: TcpStream, width: u16, height: u16) -> Self {
        // inner.set_nodelay(true).unwrap();

        let buffer_size =
            height as usize * width as usize * size_of_val(b"PX xxxxx yyyyy ffffff\n");
        let buffer = unsafe { Box::new_zeroed_slice(buffer_size).assume_init() };

        let mut px_rgb_cache = Box::new_uninit_slice(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                let key = Self::pixel_key(x, y, width);
                px_rgb_cache[key].write(PxRgbTemplate::new(x, y));
            }
        }
        let px_rgb_cache = unsafe { px_rgb_cache.assume_init() };

        Self {
            inner,
            buffer,
            buffer_len: 0,
            px_rgb_cache,
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
    fn write_px_rgb(&mut self, x: u16, y: u16, rgb: [u8; 3]) {
        let key = Self::pixel_key(x, y, self.width);
        let template = &unsafe { self.px_rgb_cache.get_unchecked(key) };
        let len = template.len as usize;

        let start = self.buffer_len;
        let end = start + len;
        let out = &mut self.buffer[start..end];

        out.copy_from_slice(unsafe { template.bytes.get_unchecked(..len) });

        let offset = template.rgb_offset as usize;
        static CHARS: &[u8; 16] = b"0123456789abcdef";
        let [r, g, b] = rgb;
        unsafe {
            *out.get_unchecked_mut(offset + 0) = CHARS[(r / 16) as usize];
            *out.get_unchecked_mut(offset + 1) = CHARS[(r % 16) as usize];
            *out.get_unchecked_mut(offset + 2) = CHARS[(g / 16) as usize];
            *out.get_unchecked_mut(offset + 3) = CHARS[(g % 16) as usize];
            *out.get_unchecked_mut(offset + 4) = CHARS[(b / 16) as usize];
            *out.get_unchecked_mut(offset + 5) = CHARS[(b % 16) as usize];
        }

        self.buffer_len += len;
    }

    fn offset(&mut self, offset_x: u16, offset_y: u16) -> io::Result<()> {
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        self.inner
            .write_all(format!("OFFSET {offset_x} {offset_y}\n").as_bytes())
    }

    #[inline(never)]
    fn flush(&mut self) -> io::Result<()> {
        // eprintln!("{}", String::from_utf8_lossy(&self.buffer));
        while let Err(err) = self.inner.write_all(&self.buffer[..self.buffer_len]) {
            if err.kind() != io::ErrorKind::WouldBlock {
                return Err(err);
            }
        }
        self.buffer_len = 0;
        Ok(())
    }
}

struct PxRgbTemplate {
    bytes: [u8; 3 + 5 + 1 + 5 + 1 + 6 + 1],
    len: u8,
    rgb_offset: u8,
}

impl PxRgbTemplate {
    fn new(x: u16, y: u16) -> Self {
        let mut x_buffer = [0; 5];
        let x_len = encode_u16(x, &mut x_buffer);
        let x = &x_buffer[5 - x_len..];

        let mut y_buffer = [0; 5];
        let y_len = encode_u16(y, &mut y_buffer);
        let y = &y_buffer[5 - y_len..];

        let mut bytes = [0; _];
        let mut pos = 0;

        bytes[pos..pos + 3].copy_from_slice(b"PX ");
        pos += 3;

        bytes[pos..pos + x.len()].copy_from_slice(x);
        pos += x.len();

        bytes[pos] = b' ';
        pos += 1;

        bytes[pos..pos + y.len()].copy_from_slice(y);
        pos += y.len();

        bytes[pos] = b' ';
        pos += 1;

        // Skip rgb hex section
        let rgb_offset = pos;
        pos += 6;

        bytes[pos] = b'\n';
        pos += 1;

        Self {
            bytes,
            len: pos as u8,
            rgb_offset: rgb_offset as u8,
        }
    }
}

fn encode_u16(value: u16, buffer: &mut [u8; 5]) -> usize {
    buffer[0] = b'0' + (value / 10_000 % 10) as u8;
    buffer[1] = b'0' + (value / 1_000 % 10) as u8;
    buffer[2] = b'0' + (value / 100 % 10) as u8;
    buffer[3] = b'0' + (value / 10 % 10) as u8;
    buffer[4] = b'0' + (value / 1 % 10) as u8;

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

fn set_pixel(pixels: &mut [u8; PIXEL_SIZE * WIDTH * HEIGHT], x: usize, y: usize, rgb: [u8; 3]) {
    let start = (x + WIDTH * y) * PIXEL_SIZE;
    let [r, g, b] = rgb;

    pixels[start] = r;
    pixels[start + 1] = g;
    pixels[start + 2] = b;
}

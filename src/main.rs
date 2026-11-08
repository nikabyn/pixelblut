#![allow(static_mut_refs)]

use std::boxed::BoxedArrayIntoIter;
use std::io::{self, prelude::*};
use std::net::TcpStream;
use std::thread::{self, sleep};
use std::time::Duration;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const PIXEL_SIZE: usize = 3;
const PIXEL_BUFFER_SIZE: usize = PIXEL_SIZE * WIDTH * HEIGHT;
static mut PIXEL_BUFFER: [u8; PIXEL_BUFFER_SIZE] = [0; _];

fn main() -> std::io::Result<()> {
    let colors = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [255, 0, 255],
        [0, 255, 255],
    ];

    for mut chunk in chunks(16) {
        thread::spawn(move || {
            let stream = TcpStream::connect("10.42.0.1:1234").unwrap();
            let mut stream = PixelflutStream::new(stream);

            // eprintln!("u16 cache size = {}", size_of_val(&*stream.u16_cache));

            let rows_start = chunk.rows_start;
            let color = colors[chunk.index % colors.len()];
            for i in 0.. {
                for (row_index, row) in chunk.rows().enumerate() {
                    let y = row_index + rows_start;
                    for (x, rgb) in unsafe { row.as_chunks_unchecked::<PIXEL_SIZE>() }
                        .iter()
                        .enumerate()
                    {
                        stream.write_px_rgb(
                            x as u16,
                            y as u16,
                            if i % 2 == 0 { color } else { *rgb },
                        );
                    }

                    stream.flush().unwrap();
                }
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

struct EncodedU16 {
    bytes: [u8; 5],
    len: u8,
}

impl EncodedU16 {
    fn new(value: u16) -> Self {
        let mut bytes = [b' '; 5];
        let len = encode_u16(value, &mut bytes);
        Self {
            bytes,
            len: len as u8,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY len is always in range 1..=5
        &unsafe { self.bytes.get_unchecked(5 - self.len as usize..) }
    }
}

struct PixelflutStream {
    inner: TcpStream,
    buffer: [u8; Self::BUFFER_SIZE],
    buffer_len: usize,
    u16_cache: Box<[EncodedU16; 65536]>,
    prefix_cache: Box<[(); 0]>,
}

impl PixelflutStream {
    const BUFFER_SIZE: usize = WIDTH * size_of_val(b"PX xxxxx yyyyy ffffff\n");

    fn new(inner: TcpStream) -> Self {
        Self {
            inner,
            buffer: [0; _],
            buffer_len: 0,
            u16_cache: {
                let array = Box::<[EncodedU16; 65536]>::new_uninit();

                unsafe {
                    let ptr = Box::into_raw(array).cast::<EncodedU16>();

                    for i in 0..=u16::MAX {
                        ptr.add(i as usize).write(EncodedU16::new(i));
                    }

                    Box::from_raw(ptr.cast::<[EncodedU16; _]>())
                }
            },
            prefix_cache: Box::new([]),
        }
    }

    #[inline(never)]
    fn write_px_rgb(&mut self, x: u16, y: u16, rgb: [u8; 3]) {
        // SAFETY: values are u16 and must be less than 65536
        let x = unsafe { self.u16_cache.get_unchecked(x as usize) }.as_bytes();
        let y = unsafe { self.u16_cache.get_unchecked(y as usize) }.as_bytes();

        let len =
            size_of_val(b"PX ") + x.len() + size_of_val(b" ") + y.len() + size_of_val(" rrggbb\n");

        let start = self.buffer_len;
        let end = start + len;
        let out = &mut self.buffer[start..end];
        let mut pos = 0;

        out[pos..pos + 3].copy_from_slice(b"PX ");
        pos += 3;

        out[pos..pos + x.len()].copy_from_slice(x);
        pos += x.len();

        out[pos] = b' ';
        pos += 1;

        out[pos..pos + y.len()].copy_from_slice(y);
        pos += y.len();

        out[pos] = b' ';
        pos += 1;

        encode_rgb_hex(rgb, {
            let hex_buffer = out[pos..pos + 6].as_mut_array();
            // SAFETY slice has constant length 6
            unsafe { hex_buffer.unwrap_unchecked() }
        });
        pos += 6;

        out[pos] = b'\n';
        pos += 1;

        self.buffer_len += pos;
    }

    fn flush(&mut self) -> io::Result<()> {
        // eprintln!("{}", String::from_utf8_lossy(&self.buffer));
        self.inner.write_all(&self.buffer[..self.buffer_len])?;
        self.buffer_len = 0;
        Ok(())
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

fn set_pixel(pixels: &mut [u8; PIXEL_SIZE * WIDTH * HEIGHT], x: usize, y: usize, rgb: [u8; 3]) {
    let start = (x + WIDTH * y) * PIXEL_SIZE;
    let [r, g, b] = rgb;

    pixels[start] = r;
    pixels[start + 1] = g;
    pixels[start + 2] = b;
}

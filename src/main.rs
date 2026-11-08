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

    for mut chunk in chunks(240) {
        thread::spawn(move || {
            let stream = TcpStream::connect("10.42.0.1:1234").unwrap();
            let mut stream = BufTcpStream::new(stream);

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
                        stream.write_bytes(b"PX ");
                        stream.write_u16(x as u16);
                        stream.write_byte(b' ');
                        stream.write_u16(y as u16);
                        stream.write_byte(b' ');
                        stream.write_rgb_hex(if i % 2 == 0 { color } else { *rgb });
                        stream.write_byte(b'\n');
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
    fn new(mut value: u16) -> Self {
        let mut bytes = [b' '; 5];
        let mut len = 0;

        loop {
            bytes[len] = b'0' + (value % 10) as u8;
            len += 1;
            value /= 10;

            if value == 0 {
                break;
            }
        }

        bytes[..len].reverse();

        Self {
            bytes,
            len: len as u8,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY len is always in range 1..=5
        &unsafe { self.bytes.get_unchecked(..self.len as usize) }
    }
}

struct BufTcpStream {
    inner: TcpStream,
    buffer: [u8; Self::BUFFER_SIZE],
    buffer_len: usize,
    u16_cache: Box<[EncodedU16; 65536]>,
}

impl BufTcpStream {
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
        }
    }

    #[inline(never)]
    fn write_u16(&mut self, value: u16) {
        assert!(self.buffer_len + 5 < self.buffer.len());

        // SAFETY: value is u16 and must be less than 65536
        let bytes = unsafe { self.u16_cache.get_unchecked(value as usize) }.as_bytes();

        let new_len = self.buffer_len + bytes.len();

        // SAFETY: the assert at the start of the function ensures that there is enough capacity
        unsafe { self.buffer.get_unchecked_mut(self.buffer_len..new_len) }.copy_from_slice(&bytes);

        self.buffer_len = new_len;
    }

    fn write_rgb_hex(&mut self, rgb: [u8; 3]) {
        let chars = b"0123456789abcdef";

        let new_len = self.buffer_len + 6;
        let buffer = &mut self.buffer[self.buffer_len..new_len];
        let [r, g, b] = rgb;
        buffer[0] = chars[(r / 16) as usize];
        buffer[1] = chars[(r % 16) as usize];
        buffer[2] = chars[(g / 16) as usize];
        buffer[3] = chars[(g % 16) as usize];
        buffer[4] = chars[(b / 16) as usize];
        buffer[5] = chars[(b % 16) as usize];

        self.buffer_len = new_len;
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        let new_len = self.buffer_len + bytes.len();
        self.buffer[self.buffer_len..new_len].copy_from_slice(bytes);
        self.buffer_len = new_len;
    }

    fn write_byte(&mut self, byte: u8) {
        self.buffer[self.buffer_len] = byte;
        self.buffer_len += 1;
    }

    fn flush(&mut self) -> io::Result<()> {
        // eprintln!("{}", String::from_utf8_lossy(&self.buffer));
        self.inner.write_all(&self.buffer[..self.buffer_len])?;
        self.buffer_len = 0;
        Ok(())
    }
}

fn set_pixel(pixels: &mut [u8; PIXEL_SIZE * WIDTH * HEIGHT], x: usize, y: usize, rgb: [u8; 3]) {
    let start = (x + WIDTH * y) * PIXEL_SIZE;
    let [r, g, b] = rgb;

    pixels[start] = r;
    pixels[start + 1] = g;
    pixels[start + 2] = b;
}

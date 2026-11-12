use std::mem::transmute;

use crate::Pixels;

pub struct Encoder {
    cache: Box<[u8]>,
    offsets: Box<[usize]>,
    width: u16,
    height: u16,
    interlacing: Option<Interlacing>,
}

impl Encoder {
    pub fn new(width: u16, height: u16, interlacing: Option<Interlacing>) -> Self {
        let elements = if interlacing.is_some() {
            (width as usize * height as usize) / 2
        } else {
            width as usize * height as usize
        };
        let mut cache = Vec::with_capacity(elements * size_of_val(b"PX xxxxx yyyyy ffffff\n"));
        let mut offsets = Vec::with_capacity(elements);

        for y in 0..height {
            for x in 0..width {
                if let Some(interlacing) = interlacing
                    && ((x ^ y) & 1) == interlacing as u16
                {
                    continue;
                }
                cache.extend_from_slice(format!("PX {x} {y} ").as_bytes());

                // Skip rgb hex section
                offsets.push(cache.len());
                cache.extend_from_slice(b"ff00ff\n");
            }
        }

        cache.shrink_to_fit();

        Self {
            cache: cache.into_boxed_slice(),
            offsets: offsets.into_boxed_slice(),
            width,
            height,
            interlacing,
        }
    }

    pub fn update(&mut self, pixels: &Pixels) {
        assert!(pixels.width == self.width && pixels.height() == self.height);

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

        let mut cache_index = 0;
        for (i, [r, g, b]) in pixels.as_slice().iter().enumerate() {
            let x = i % self.width as usize;
            let y = i / self.width as usize;

            if let Some(interlacing) = self.interlacing
                && ((x ^ y) & 1) == interlacing as usize
            {
                continue;
            }

            let offset = self.offsets[cache_index];
            cache_index += 1;

            let dst: &mut [u16] = unsafe { transmute(&mut self.cache[offset..]) };
            dst[0] = HEX_U8[*r as usize];
            dst[1] = HEX_U8[*g as usize];
            dst[2] = HEX_U8[*b as usize];
        }
    }

    pub fn encoded(&self) -> &[u8] {
        &self.cache
    }
}

#[derive(Clone, Copy)]
pub enum Interlacing {
    Even = 0,
    Odd = 1,
}

pub fn encode_u16(value: u16, buffer: &mut [u8; 5]) -> usize {
    buffer[0] = b'0' + (value / 10_000 % 10) as u8;
    buffer[1] = b'0' + (value / 1_000 % 10) as u8;
    buffer[2] = b'0' + (value / 100 % 10) as u8;
    buffer[3] = b'0' + (value / 10 % 10) as u8;
    buffer[4] = b'0' + (value / 1 % 10) as u8;

    digits_u16(value)
}

pub fn digits_u16(value: u16) -> usize {
    match value {
        ..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        1_000..=9_999 => 4,
        10_000.. => 5,
    }
}

pub fn encode_rgb_hex(rgb: [u8; 3], buffer: &mut [u8; 6]) {
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
    use std::time::{Duration, Instant};

    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    let mut pixels = Pixels::new(WIDTH as u16, HEIGHT as u16, [0; 3]);

    for (i, rgb) in pixels.as_slice_mut().iter_mut().enumerate() {
        for byte in rgb {
            *byte = match i {
                0..12 => 0x0f,
                15..52 => 0x12,
                60..80 => 0xff,
                100..200 => i as u8,
                _ => 0x00,
            };
        }
    }

    let mut encoder = Encoder::new(WIDTH as u16, HEIGHT as u16, None);

    let iterations = 500;
    let mut encode_times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        encoder.update(&pixels);
        encode_times.push(start.elapsed());

        for (i, rgb) in pixels.as_slice_mut().iter_mut().enumerate() {
            for byte in rgb {
                *byte = match i {
                    0..100 => 255 - *byte,
                    100..200 => *byte - (i % 255) as u8,
                    _ => *byte % 125,
                };
            }
        }
    }

    println!("{:?}", encoder.encoded().iter().sum::<u8>());
    let average_encode_t: Duration =
        encode_times.into_iter().reduce(|a, b| a + b).unwrap() / iterations;
    let average_encode_fps = Duration::from_secs(1).as_nanos() / average_encode_t.as_nanos();
    println!("average: {average_encode_t:?} per frame, {average_encode_fps:?}fps");
}

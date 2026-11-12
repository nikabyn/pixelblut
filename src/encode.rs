use std::mem::transmute;

use crate::Pixels;

pub trait TextEncoder {
    fn init(&self, width: u16, height: u16) -> (Box<[u8]>, Box<[usize]>) {
        let elements = Self::cache_len(width, height);
        let mut cache = Vec::with_capacity(elements * size_of_val(b"PX xxxxx yyyyy ffffff\n"));
        let mut offsets = Vec::with_capacity(elements);

        for y in 0..height {
            for x in 0..width {
                if self.skip_coord(x, y) {
                    continue;
                }
                cache.extend_from_slice(format!("PX {x} {y} ").as_bytes());

                // Skip rgb hex section
                offsets.push(cache.len());
                cache.extend_from_slice(b"ff00ff\n");
            }
        }

        cache.shrink_to_fit();

        (cache.into_boxed_slice(), offsets.into_boxed_slice())
    }

    fn encode(&mut self, pixels: &Pixels) -> &[u8] {
        assert!(pixels.width == self.width() && pixels.height() == self.height());

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
            let x = i % self.width() as usize;
            let y = i / self.width() as usize;
            if self.skip_coord(x as u16, y as u16) {
                continue;
            }

            let offset = self.offsets()[cache_index];
            cache_index += 1;

            let dst: &mut [u16] = unsafe { transmute(&mut self.cache_mut().as_mut()[offset..]) };
            dst[0] = HEX_U8[*r as usize];
            dst[1] = HEX_U8[*g as usize];
            dst[2] = HEX_U8[*b as usize];
        }

        self.cache()
    }

    fn cache_len(width: u16, height: u16) -> usize;
    fn skip_coord(&self, x: u16, y: u16) -> bool;

    fn width(&self) -> u16;
    fn height(&self) -> u16;
    fn cache(&self) -> &[u8];
    fn cache_mut(&mut self) -> &mut [u8];
    fn offsets(&mut self) -> &[usize];
}

pub struct BasicTextEncoder {
    cache: Box<[u8]>,
    offsets: Box<[usize]>,
    width: u16,
    height: u16,
}

impl BasicTextEncoder {
    pub fn new(width: u16, height: u16) -> Self {
        let (cache, offsets) = Self {
            cache: Box::new([]),
            offsets: Box::new([]),
            width,
            height,
        }
        .init(width, height);

        Self {
            cache,
            offsets,
            width,
            height,
        }
    }
}

impl TextEncoder for BasicTextEncoder {
    fn cache_len(width: u16, height: u16) -> usize {
        width as usize * height as usize
    }

    fn skip_coord(&self, _x: u16, _y: u16) -> bool {
        false
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn cache(&self) -> &[u8] {
        &self.cache
    }

    fn cache_mut(&mut self) -> &mut [u8] {
        &mut self.cache
    }

    fn offsets(&mut self) -> &[usize] {
        &self.offsets
    }
}

pub struct InterlacedTextEncoder {
    cache: Box<[u8]>,
    offsets: Box<[usize]>,
    width: u16,
    height: u16,
    offset: u8,
}

impl InterlacedTextEncoder {
    pub fn new(width: u16, height: u16, offset: u8) -> Self {
        let (cache, offsets) = Self {
            cache: Box::new([]),
            offsets: Box::new([]),
            width,
            height,
            offset,
        }
        .init(width, height);

        Self {
            cache,
            offsets,
            width,
            height,
            offset,
        }
    }
}

impl TextEncoder for InterlacedTextEncoder {
    fn cache_len(width: u16, height: u16) -> usize {
        (width as usize * height as usize) / 2
    }

    fn skip_coord(&self, x: u16, y: u16) -> bool {
        ((x ^ y) & 1) == self.offset as u16
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn cache(&self) -> &[u8] {
        &self.cache
    }

    fn cache_mut(&mut self) -> &mut [u8] {
        &mut self.cache
    }

    fn offsets(&mut self) -> &[usize] {
        &self.offsets
    }
}

// pub fn encode_u16(value: u16, buffer: &mut [u8; 5]) -> usize {
//     buffer[0] = b'0' + (value / 10_000 % 10) as u8;
//     buffer[1] = b'0' + (value / 1_000 % 10) as u8;
//     buffer[2] = b'0' + (value / 100 % 10) as u8;
//     buffer[3] = b'0' + (value / 10 % 10) as u8;
//     buffer[4] = b'0' + (value / 1 % 10) as u8;
//
//     digits_u16(value)
// }
//
// pub fn digits_u16(value: u16) -> usize {
//     match value {
//         ..=9 => 1,
//         10..=99 => 2,
//         100..=999 => 3,
//         1_000..=9_999 => 4,
//         10_000.. => 5,
//     }
// }
//
// pub fn encode_rgb_hex(rgb: [u8; 3], buffer: &mut [u8; 6]) {
//     let chars = b"0123456789abcdef";
//
//     let [r, g, b] = rgb;
//     buffer[0] = chars[(r / 16) as usize];
//     buffer[1] = chars[(r % 16) as usize];
//     buffer[2] = chars[(g / 16) as usize];
//     buffer[3] = chars[(g % 16) as usize];
//     buffer[4] = chars[(b / 16) as usize];
//     buffer[5] = chars[(b % 16) as usize];
// }

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

    let mut encoder = BasicTextEncoder::new(WIDTH as u16, HEIGHT as u16);

    let iterations = 500;
    let mut encode_times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        encoder.encode(&pixels);
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

    println!("{:?}", encoder.cache().iter().sum::<u8>());
    let average_encode_t: Duration =
        encode_times.into_iter().reduce(|a, b| a + b).unwrap() / iterations;
    let average_encode_fps = Duration::from_secs(1).as_nanos() / average_encode_t.as_nanos();
    println!("average: {average_encode_t:?} per frame, {average_encode_fps:?}fps");
}

pub struct PxFrameEncoder {
    cache: Box<[u8]>,
    offsets: Box<[usize]>,
    width: u16,
    height: u16,
}

impl PxFrameEncoder {
    pub fn new(width: u16, height: u16) -> Self {
        let mut cache = Vec::with_capacity(
            width as usize * height as usize * size_of_val(b"PX xxxxx yyyyy ffffff\n"),
        );
        let mut offsets = Vec::with_capacity(width as usize * height as usize);

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
                offsets.push(cache.len());
                cache.extend_from_slice(b"000000");

                cache.push(b'\n');
            }
        }

        cache.shrink_to_fit();

        Self {
            cache: cache.into_boxed_slice(),
            offsets: offsets.into_boxed_slice(),
            width,
            height,
        }
    }

    pub unsafe fn update_row(&mut self, row_y: u16, row_data: &[[u8; 3]]) {
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

        let start_key = pixel_key(0, row_y, self.width);

        let mut offset = unsafe { *self.offsets.get_unchecked(start_key) };
        let mut stride = size_of_val(b"PX   ffffff\n").wrapping_add(digits_u16(row_y));

        for (x, [r, g, b]) in row_data.iter().enumerate() {
            let inc_stride = x == 0 || x == 9 || x == 99 || x == 999 || x == 9999;
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

    pub fn encoded(&self) -> &[u8] {
        &self.cache
    }
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

pub fn pixel_key(x: u16, y: u16, width: u16) -> usize {
    y as usize * width as usize + x as usize
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
    let mut pixels = Box::new([[0; 3]; WIDTH * HEIGHT]);

    for (i, rgb) in pixels.iter_mut().enumerate() {
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

    let mut encoder = PxFrameEncoder::new(WIDTH as u16, HEIGHT as u16);

    let iterations = 500;
    let mut encode_times = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        for (row_y, row_data) in pixels.chunks_exact_mut(WIDTH).enumerate() {
            unsafe { encoder.update_row(row_y as u16, row_data) };
        }
        encode_times.push(start.elapsed());

        for (i, rgb) in pixels.iter_mut().enumerate() {
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

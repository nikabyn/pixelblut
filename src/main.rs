#![allow(static_mut_refs)]

use std::io::{BufWriter, prelude::*};
use std::net::TcpStream;
use std::thread;

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
static mut PIXEL_BUFFER: [u8; 3 * WIDTH * HEIGHT] = [0; _];

fn get_pixel(pixels: &[u8; 3 * WIDTH * HEIGHT], x: usize, y: usize) -> [u8; 3] {
    let start = (x + WIDTH * y) * 3;

    let r = pixels[start];
    let g = pixels[start + 1];
    let b = pixels[start + 2];

    [r, g, b]
}

fn set_pixel(pixels: &mut [u8; 3 * WIDTH * HEIGHT], x: usize, y: usize, rgb: [u8; 3]) {
    let start = (x + WIDTH * y) * 3;
    let [r, g, b] = rgb;

    pixels[start] = r;
    pixels[start + 1] = g;
    pixels[start + 2] = b;
}

fn main() -> std::io::Result<()> {
    const NUM_THREADS: usize = 20;

    for thread_index in 0..NUM_THREADS {
        thread::spawn(move || {
            let mut stream = TcpStream::connect("10.42.0.1:1234").unwrap();

            const BUFFER_SIZE: usize =
                (HEIGHT / NUM_THREADS) * WIDTH * size_of_val(b"PX xxxx yyyy ffffff\n");
            // let mut output_buffer: [u8; BUFFER_SIZE] = [0; _];
            // let mut output_start: usize = 0;
            let mut stream = BufWriter::with_capacity(BUFFER_SIZE, stream);

            loop {
                // eprintln!("Sending frame");

                for y in (thread_index..HEIGHT).step_by(NUM_THREADS) {
                    for x in 0..WIDTH {
                        let [r, g, b] = get_pixel(unsafe { &PIXEL_BUFFER }, x, y);
                        writeln!(stream, "PX {x} {y} {r:02x}{g:02x}{b:02x}").unwrap();
                    }
                }

                stream.flush().unwrap();
            }
        });
    }

    let mut i: usize = 0;
    loop {
        i = i.wrapping_add(1);
        let t = (i % 1000) as f32 / 1000.0;
        // eprintln!("Drawing frame");

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

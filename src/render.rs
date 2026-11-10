use std::{num::NonZeroU32, time::Instant};

pub fn run_render_loop(pixels: *mut [u8; 3], width: usize, height: usize) -> ! {
    let max_fps = NonZeroU32::new(60).unwrap();
    let clock = chron::Clock::new(NonZeroU32::new(120).unwrap()).max_frame_rate(max_fps);

    let start = Instant::now();
    for tick in clock {
        if matches!(tick, chron::Tick::Update) {
            continue;
        }

        let elapsed = start.elapsed().as_secs_f32();
        let t = (elapsed % 1.0) / 1.0;

        render_frame(pixels, t, width, height);
    }

    unreachable!()
}

fn render_frame(pixels: *mut [u8; 3], t: f32, width: usize, height: usize) {
    let box_center_x = (width as i32 / 2 + ((width / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
    let box_center_y =
        (height as i32 / 2 - ((height / 2) as f32 * (t * 2.0 - 1.0)) as i32) as usize;
    let box_width = height / 10;
    let box_height = box_width;
    let box_x = (box_center_x - box_width / 2)..(box_center_x + box_width / 2);
    let box_y = (box_center_y - box_height / 2)..(box_center_y + box_height / 2);

    for y in 0..height {
        let base = unsafe { pixels.add(y * width) };

        let v = (y as f32 / height as f32 * 255.0) as u8;
        for x in 0..width {
            let [r, g, b] = unsafe { base.add(x).as_mut_unchecked() };
            let u = (x as f32 / width as f32 * 255.0) as u8;
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

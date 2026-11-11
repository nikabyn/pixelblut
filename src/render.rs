pub struct FrameRenderer {
    pub pixels: Box<[[u8; 3]]>,
    width: u16,
    height: u16,
}

impl FrameRenderer {
    pub fn new(width: u16, height: u16) -> Self {
        let pixels_uninit = Box::new_uninit_slice(width as usize * height as usize);
        Self {
            // SAFETY Pixels always have a valid state.
            //        The only way to access them is through render,
            //        which overrides all of them.
            pixels: unsafe { pixels_uninit.assume_init() },
            width,
            height,
        }
    }

    pub fn render(&mut self, t: f32) -> &[[u8; 3]] {
        let t = (t % 3.0) / 3.0;

        let half_width = self.width / 2;
        let half_height = self.height / 2;

        let box_center_x =
            (half_width as i32 + (half_width as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_center_y =
            (half_height as i32 - (half_height as f32 * (t * 2.0 - 1.0)) as i32) as usize;
        let box_width = self.height as usize / 10;
        let box_height = box_width;
        let box_x = (box_center_x - box_width / 2)..(box_center_x + box_width / 2);
        let box_y = (box_center_y - box_height / 2)..(box_center_y + box_height / 2);

        for y in 0..(self.height as usize) {
            let base = unsafe { self.pixels.as_mut_ptr().add(y * self.width as usize) };

            let v = (y as f32 / self.height as f32 * 255.0) as u8;
            for x in 0..(self.width as usize) {
                let [r, g, b] = unsafe { base.add(x).as_mut_unchecked() };
                let u = (x as f32 / self.width as f32 * 255.0) as u8;
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

        &self.pixels
    }
}

use minifb::{Scale, Window, WindowOptions};
use xteink_render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DISPLAY_WIDTH_BYTES, Framebuffer};

pub struct SimulatorWindow {
    window: Window,
    pixels: Vec<u32>,
    scale: usize,
}

impl SimulatorWindow {
    pub fn new(title: &str, scale: usize) -> Result<Self, minifb::Error> {
        let width = usize::from(DISPLAY_WIDTH) * scale;
        let height = usize::from(DISPLAY_HEIGHT) * scale;
        let window = Window::new(
            title,
            width,
            height,
            WindowOptions {
                scale: Scale::X1,
                resize: false,
                ..WindowOptions::default()
            },
        )?;
        Ok(Self {
            window,
            pixels: vec![0x00FF_FFFF; width * height],
            scale,
        })
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    pub fn update(&mut self, framebuffer: &Framebuffer) -> Result<(), minifb::Error> {
        let width = usize::from(DISPLAY_WIDTH);
        let height = usize::from(DISPLAY_HEIGHT);
        let scaled_width = width * self.scale;

        for y in 0..height {
            for x in 0..width {
                let py = width - 1 - x;
                let idx = py * usize::from(DISPLAY_WIDTH_BYTES) + (y / 8);
                let bit = 7 - (y as u16 % 8);
                let black = (framebuffer.bytes()[idx] & (1 << bit)) == 0;
                let color = if black { 0x00000000 } else { 0x00FF_FFFF };

                let start_x = x * self.scale;
                let start_y = y * self.scale;
                for sy in 0..self.scale {
                    let row = (start_y + sy) * scaled_width;
                    for sx in 0..self.scale {
                        self.pixels[row + start_x + sx] = color;
                    }
                }
            }
        }

        self.window
            .update_with_buffer(&self.pixels, width * self.scale, height * self.scale)
    }
}

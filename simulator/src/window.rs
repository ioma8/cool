use minifb::{Scale, Window, WindowOptions};
use xteink_render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Framebuffer, SHADE_BLACK};

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
                let color = shade_to_rgb(framebuffer.shade_at(x as u16, y as u16));

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

fn shade_to_rgb(shade: u8) -> u32 {
    let value: u8 = match shade.min(SHADE_BLACK) {
        0 => 0xFF,
        1 => 0xAA,
        2 => 0x55,
        _ => 0x00,
    };
    (u32::from(value) << 16) | (u32::from(value) << 8) | u32::from(value)
}

#[cfg(test)]
mod tests {
    use super::shade_to_rgb;

    #[test]
    fn shade_mapping_uses_four_distinct_grayscale_values() {
        assert_eq!(shade_to_rgb(0), 0x00FF_FFFF);
        assert_eq!(shade_to_rgb(1), 0x00AA_AAAA);
        assert_eq!(shade_to_rgb(2), 0x0055_5555);
        assert_eq!(shade_to_rgb(3), 0x0000_0000);
    }
}

pub trait DemoDisplay {
    type Error;

    fn init(&mut self);
    fn clear(&mut self, color: u8);
    fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error>;
    fn refresh_full(&mut self);
}

pub fn show_embedded_epub_demo<D: DemoDisplay>(display: &mut D) -> Result<(), D::Error> {
    display.init();
    display.clear(0xFF);
    display.render_embedded_epub_first_screen()?;
    display.refresh_full();
    Ok(())
}

use std::time::Instant;

pub struct Timer<'a> {
    name: &'a str,
    start: Instant
}

impl<'a> Timer<'a> {
    pub fn new(name: &'a str) -> Self {
        Self { 
            name,
            start: Instant::now()
        }
    }

    pub fn report_elapsed(&self) {
        println!("{}: {:.3}s", self.name, self.start.elapsed().as_secs_f32());
    }
}

impl<'a> Drop for Timer<'a> {
    fn drop(&mut self) {
        self.report_elapsed();
    }
}
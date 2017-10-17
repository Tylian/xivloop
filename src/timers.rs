extern crate time;

use std::collections::HashMap;

pub struct Timer {
    timers: HashMap<&'static str, f64>,
}

impl Timer {
    pub fn new() -> Timer {
        Timer { timers: HashMap::new() }
    }

    pub fn start(&mut self, name: &'static str) {
        self.timers.insert(name, time::precise_time_s());
    }

    pub fn report(&self, name: &str) {
        match self.get_elapsed(name) {
            Some(elapsed) => println!("{}: {:.3}s", name, elapsed),
            None => panic!("Timer {} not found!", name),
        }
    }

    pub fn get_elapsed(&self, name: &str) -> Option<f64> {
        match self.timers.get(name) {
            Some(start) => Some(time::precise_time_s() - start),
            None => None,
        }
    }

    pub fn report_with<F>(&self, name: &str, f: F)
    where
        F: Fn(Option<f64>),
    {
        f(self.get_elapsed(name))
    }
}

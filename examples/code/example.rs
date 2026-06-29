// A small Rust example: a typed counter with a saturating increment.

struct Counter {
    value: u32,
    limit: u32,
}

impl Counter {
    fn new(limit: u32) -> Self {
        Counter { value: 0, limit }
    }

    fn increment(&mut self) -> bool {
        if self.value < self.limit {
            self.value += 1;
            true
        } else {
            false
        }
    }
}

fn main() {
    let mut c = Counter::new(3);
    while c.increment() {
        println!("count = {}", c.value);
    }
    println!("reached limit of {}", c.limit);
}

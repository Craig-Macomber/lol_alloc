use lol_alloc::SimpleAllocator;

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator::new();

fn main() {
    println!("Hello, World!");

    // Let's create a vec, and add a bunch of things to it - forcing some
    // allocations
    let mut v = vec![];
    for n in 0..(1024 * 1024) {
        println!("Pushing {}", n);
        v.push(n);
    }
}

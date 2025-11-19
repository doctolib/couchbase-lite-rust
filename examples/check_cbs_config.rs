mod utils;

use utils::*;

fn main() {
    println!("=== CBS Configuration Check ===\n");

    println!("Checking current metadata purge interval configuration:");
    get_metadata_purge_interval();

    println!("\n=== Check complete ===");
}

//! `cargo run -p rules-airline -- ⟨command⟩`:
//!
//! - `split`: prints the dev cases, for `eval/rules/airline/split.txt`.

use rules_airline::{data, data_dir, split};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("split") => print_split(),
        _ => Err("usage: rules-airline split".into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn print_split() -> Result<(), String> {
    println!("# dev cases: keyed SipHash of the case id is 0 mod 5 (src/split.rs); every other case is test");
    for c in data::cases(&data_dir())? {
        if split::is_dev(&c.id) {
            println!("{}", c.id);
        }
    }
    Ok(())
}

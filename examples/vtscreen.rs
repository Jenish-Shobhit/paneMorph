//! Render raw terminal output into screen text (a live-test helper).
//!
//! Usage: cargo run --example vtscreen -- ROWS COLS < output.bin

use std::io::Read;

fn main() {
    let args: Vec<u16> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("ROWS COLS"))
        .collect();
    let (rows, cols) = (args[0], args[1]);
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes);
    for row in parser.screen().rows(0, cols) {
        println!("{}", row.trim_end());
    }
}

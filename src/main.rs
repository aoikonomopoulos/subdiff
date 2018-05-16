extern crate lcs_diff;

use self::lcs_diff::*;
use std::env;
use std::io;
use std::io::prelude::*;
use std::fs::File;

fn read_lines(p : &str) -> io::Result<Vec<String>> {
    let f = File::open(p)?;
    let f = io::BufReader::new(f);
    f.lines().collect::<io::Result<Vec<String>>>()
}

fn main() {
    let args : Vec<String> = env::args().collect();
    let old = &args[1];
    let new = &args[2];

    let old_lines = read_lines(old).unwrap();
    let new_lines = read_lines(new).unwrap();

    for diff in lcs_diff::diff(&old_lines, &new_lines) {
        match diff {
            DiffResult::Added(a) => println!("+{} new index = {}", a.data, a.new_index.unwrap()),
            DiffResult::Common(c) => {
                println!(" {} old index = {}, new index = {}",
                         c.data,
                         c.old_index.unwrap(),
                         c.new_index.unwrap())
            }
            DiffResult::Removed(r) => println!("-{} old index = {}", r.data, r.old_index.unwrap()),
        }
    }
}

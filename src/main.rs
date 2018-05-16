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

fn display_diff_unified(diff : &Vec<DiffResult<String>>) {
    // When lines are changed, lcs_diff returns the adds before the removes.
    // However, we want to follow the practice of most diff programs and
    // print out the removes before the adds. So we set aside any consecutive
    // additions and print them (a) immediately, when we run into a common line
    // (b) after any number of consecutive removals.
    let mut pending_adds = vec![];
    let mut corked = true;
    for d in diff {
        match d {
            DiffResult::Added(a) => {
                if !corked {
                    // The pending adds are uncorked when there's an
                    // intervening remove. If so, we should drain the
                    // the pending adds before adding the new
                    // (current) one
                    for pa in pending_adds.drain(..) {
                        println!("+{}", pa)
                    }
                }
                // Any following adds should be added to the queue
                corked = true;
                pending_adds.push(&a.data)
            },
            DiffResult::Common(c) => {
                // Adds are only pending while there is the possibility
                // that they will be followed by removals. As this is
                // a common line, we need to drain them now.
                for pa in pending_adds.drain(..) {
                    println!("+{}", pa)
                }
                println!(" {}", c.data)
            },
            DiffResult::Removed(r) => {
                // Pop the cork; we've seen a remove, so any subsequent
                // diff result that is not a remove should cause the
                // pending adds to be dumped.
                corked = false;
                println!("-{}", r.data)
            },
        }
    }
    for pa in pending_adds.drain(..) {
        println!("+{}", pa)
    }
}

fn main() {
    let args : Vec<String> = env::args().collect();
    let old = &args[1];
    let new = &args[2];

    let old_lines = read_lines(old).unwrap();
    let new_lines = read_lines(new).unwrap();

    let diff : Vec<DiffResult<String>> = lcs_diff::diff(&old_lines, &new_lines);
    display_diff_unified(&diff);
}

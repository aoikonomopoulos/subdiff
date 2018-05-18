extern crate lcs_diff;
#[cfg(test)]
#[macro_use]
extern crate itertools;
#[cfg(test)]
extern crate temporary;

use self::lcs_diff::*;
use std::env;
use std::io;
use std::io::prelude::*;
use std::fs::File;
use std::path::Path;
use std::process::exit;

fn read_lines(p : &Path) -> io::Result<Vec<String>> {
    let f = File::open(p)?;
    let f = io::BufReader::new(f);
    f.lines().collect::<io::Result<Vec<String>>>()
}

fn exist_differences(results : &[DiffResult<String>]) -> bool {
    results.iter().any(|r|
                       match r {
                           DiffResult::Common (_) => false,
                           _ => true,
                       })
}

fn display_diff_unified(out : &mut Write,
                        old_lines : &Vec<String>,
                        new_lines : &Vec<String>,
                        diff : &Vec<DiffResult<String>>) -> io::Result<i32> {
    if !exist_differences(&diff) {
        return Ok (0);
    }
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
                        writeln!(out, "+{}", pa)?;
                    }
                }
                // Any following adds should be added to the queue
                corked = true;
                pending_adds.push(&new_lines[a.new_index.unwrap()])
            },
            DiffResult::Common(c) => {
                // Adds are only pending while there is the possibility
                // that they will be followed by removals. As this is
                // a common line, we need to drain them now.
                for pa in pending_adds.drain(..) {
                    writeln!(out, "+{}", pa)?;
                }
                writeln!(out, " {}", &old_lines[c.old_index.unwrap()])?;
            },
            DiffResult::Removed(r) => {
                // Pop the cork; we've seen a remove, so any subsequent
                // diff result that is not a remove should cause the
                // pending adds to be dumped.
                corked = false;
                writeln!(out, "-{}", &old_lines[r.old_index.unwrap()])?;
            },
        }
    }
    for pa in pending_adds.drain(..) {
        writeln!(out, "+{}", pa)?;
    }
    Ok (1)
}

fn diff_files(out : &mut Write, old : &Path, new : &Path) -> io::Result<i32> {
    let old_lines = read_lines(old)?;
    let new_lines = read_lines(new)?;

    let diff : Vec<DiffResult<String>> = lcs_diff::diff(&old_lines, &new_lines);
    display_diff_unified(out, &old_lines, &new_lines, &mut &diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;
    use std::process::Command;
    use std::ffi::OsStr;

    fn skip_past_third_newline(bytes : &Vec<u8>) -> Option<usize> {
        let mut cnt = 0;
        bytes.iter().position(|&el|
                       if el == b'\n' {
                           if cnt == 2 {
                               true
                           } else {
                               cnt += 1;
                               false
                           }
                       } else {
                           false
                       }).map(|pos| pos + 1)
    }

    fn test_diff(dir : &temporary::Directory,
                 lines1 : Vec<&str>, lines2 : Vec<&str>) {
        let old_p = dir.join("old");
        let mut old = File::create(&old_p).unwrap();
        let new_p = dir.join("new");
        let mut new = File::create(&new_p).unwrap();

        for l in lines1 {
            writeln!(&mut old, "{}", l).unwrap();
        }
        old.flush().unwrap();
        for l in lines2 {
            writeln!(&mut new, "{}", l).unwrap();
        }
        new.flush().unwrap();
        let outp = Command::new("diff")
            .args(&[OsStr::new("-U"), OsStr::new("100"), old_p.as_os_str(), new_p.as_os_str()])
            .output().unwrap();
        let pos = skip_past_third_newline(&outp.stdout).unwrap_or(0);
        let diff_output = &outp.stdout[pos..];
        let mut our_output : Vec<u8> = vec![];
        diff_files(&mut our_output, &old_p, &new_p).unwrap();
        if our_output != diff_output {
            eprintln!("outputs differ! ours:");
            io::stderr().write(&our_output).unwrap();
            eprintln!("diff's:");
            io::stderr().write(diff_output).unwrap();
            panic!("Output differs to the system diff output")
        }
    }
    #[test]
    fn test_combos() {
        let lines : Vec<&str>
            = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let combos1 : Vec<Vec<&str>> = lines.iter().cloned().combinations(8).collect();
        let combos2 : Vec<Vec<&str>> = lines.iter().cloned().combinations(8).collect();
        let prod = iproduct!(combos1, combos2);
        let tmpdir = temporary::Directory::new("diff-test").unwrap();
        for p in prod {
            test_diff(&tmpdir, p.0, p.1)
        }
        tmpdir.remove().unwrap()
    }
}

fn main() {
    let args : Vec<String> = env::args().collect();
    let ecode = match diff_files(&mut io::stdout(),
                                 Path::new(&args[1]),
                                 Path::new(&args[2])) {
        Ok (ecode) => ecode,
        Err (err) => {
            eprintln!("Error comparing files: {}", err);
            2
        },
    };
    exit(ecode);
}

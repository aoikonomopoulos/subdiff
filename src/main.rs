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
use std::collections::VecDeque;

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

#[derive(Debug)]
enum State<'a> {
    // Customary diff behavior is to present any removes before immediately
    // following adds, however lcs_diff returns adds before removes. So we
    // set aside any consecutive adds and print them as soon as it's clear
    // we've observed (and emitted) all immediately following removes.
    CollectingAdds(Vec<&'a str>),

    // Hold on to the last N common lines we've seen, dump them
    // as the preceeding context if a new change (addition/removal)
    // is seen.
    // We also need to prepend a separator if there were context
    // lines we had to drop, so our state also includes the number
    // of observed common lines while in this state.
    CollectingCommonsTail(usize, VecDeque<&'a str>),

    // Accumulate up to $context lines, emit them, then switch
    // to CollectingCommonsTail.
    CollectingCommonsCorked(VecDeque<&'a str>),

    // Emit seen remove, while holding on to any pending adds (see above)
    SequentialRemoves(Vec<&'a str>),
}

fn display_diff_unified(out : &mut Write,
                        old_lines : &Vec<String>,
                        new_lines : &Vec<String>,
                        diff : &Vec<DiffResult<String>>) -> io::Result<i32> {
    use State::*;
    if !exist_differences(&diff) {
        return Ok (0); // Exit w/o producing any output
    }
    let context = 3;

    // We always need to print out a hunk header when we start, so use
    // the same code path, forcing a header to be printed by pretending
    // we've observed lots of common lines already.
    let mut state = CollectingCommonsTail(context + 1, VecDeque::new());

    for d in diff {
        eprintln!("state = {:?}", state);
        eprintln!("processing diff result: {:?}", d);
        state = match state {
            CollectingAdds(mut adds) => {
                match d {
                    DiffResult::Added(a) => {
                        adds.push(&new_lines[a.new_index.unwrap()]);
                        CollectingAdds(adds) // Still collecting adds
                    },
                    DiffResult::Removed(r) => {
                        writeln!(out, "-{}", &old_lines[r.old_index.unwrap()])?;
                        // Change states, holding on to the pending adds
                        SequentialRemoves(adds)
                    },
                    DiffResult::Common(c) => {
                        // No adjacent removes, time to print out the adds
                        for pa in adds.drain(..) {
                            writeln!(out, "+{}", pa)?;
                        }
                        let mut commons = VecDeque::new();
                        commons.push_back(&new_lines[c.new_index.unwrap()][..]);
                        // We've just seen a change; this needs to be followed by
                        // some context lines.
                        CollectingCommonsCorked(commons)
                    },
                }
            },
            CollectingCommonsTail(seen, mut commons) => {
                match d {
                    // If the state changes, print out the last N lines, possibly
                    // preceeded by a header
                    DiffResult::Added(a) => {
                        if seen > context {
                            writeln!(out, "--")?;
                        }
                        for pc in commons.drain(..) {
                            writeln!(out, " {}", pc)?;
                        }
                        let adds = vec![&new_lines[a.new_index.unwrap()][..]];
                        CollectingAdds(adds)
                    },
                    DiffResult::Removed(r) => {
                        if seen > context {
                            writeln!(out, "--")?;
                        }
                        for pc in commons.drain(..) {
                            writeln!(out, " {}", pc)?;
                        }
                        writeln!(out, "-{}", &old_lines[r.old_index.unwrap()])?;
                        SequentialRemoves(vec![])
                    },
                    DiffResult::Common(c) => {
                        commons.push_back(&new_lines[c.new_index.unwrap()]);
                        if commons.len() > context {
                            commons.pop_front();
                        }
                        CollectingCommonsTail(seen + 1, commons)
                    },
                }
            },
            CollectingCommonsCorked(mut commons) => {
                match d {
                    // State change -> print collected common lines
                    DiffResult::Added(a) => {
                        for pc in commons.drain(..) {
                            writeln!(out, " {}", pc)?;
                        }
                        let adds = vec![&new_lines[a.new_index.unwrap()][..]];
                        CollectingAdds(adds)
                    },
                    DiffResult::Removed(r) => {
                        for pc in commons.drain(..) {
                            writeln!(out, " {}", pc)?;
                        }
                        writeln!(out, "-{}", &old_lines[r.old_index.unwrap()])?;
                        SequentialRemoves(vec![])
                    },
                    DiffResult::Common(c) => {
                        commons.push_back(&new_lines[c.new_index.unwrap()]);
                        if commons.len() == context {
                            // We've accumulated $context common lines after
                            // a change; print them out, then start collecting
                            // common lines to print _before_ the next change.
                            for pc in commons.drain(..) {
                                writeln!(out, " {}", pc)?;
                            }
                            CollectingCommonsTail(0, VecDeque::new())
                        } else {
                            CollectingCommonsCorked(commons)
                        }
                    },
                }
            },
            SequentialRemoves(mut adds) => {
                match d {
                    // State change -> time to print out the pending adds
                    DiffResult::Added(a) => {
                        for pa in adds.drain(..) {
                            writeln!(out, "+{}", pa)?;
                        }
                        let adds = vec![&new_lines[a.new_index.unwrap()][..]];
                        CollectingAdds(adds)
                    },
                    DiffResult::Removed(r) => {
                        // Simply print out the remove
                        writeln!(out, "-{}", &old_lines[r.old_index.unwrap()])?;
                        SequentialRemoves(adds)
                    },
                    DiffResult::Common(c) => {
                        for pa in adds.drain(..) {
                            writeln!(out, "+{}", pa)?;
                        }
                        let mut commons = VecDeque::new();
                        // XXX: handle context = 0
                        commons.push_back(&new_lines[c.new_index.unwrap()][..]);
                        CollectingCommonsCorked(commons)
                    },
                }

            },
        }
    }
    eprintln!("Handling final state: {:?}", state);
    // Cleanup
    match state {
        // We might end up here if the last additions are
        // exactly at the end of the file.
        CollectingAdds (mut adds) => {
            for pa in adds.drain(..) {
                writeln!(out, "+{}", pa)?;
            }
        },
        // Those are common lines we collected in anticipation of the
        // next change. No change is coming any more, so drop them here.
        CollectingCommonsTail(_, _) => (),
        // We'll get here if there were < $context common lines between
        // the last change and the end of the file. We still need to
        // print them.
        CollectingCommonsCorked(mut commons) => {
            for pc in commons.drain(..) {
                writeln!(out, " {}", pc)?;
            }
        },
        // We may end up here if the last change is at the EOF.
        SequentialRemoves(mut adds) => {
            for pa in adds.drain(..) {
                writeln!(out, "+{}", pa)?;
            }
        }
    };
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

    fn skip_past_second_newline(bytes : &Vec<u8>) -> Option<usize> {
        let mut cnt = 0;
        bytes.iter().position(|&el|
                       if el == b'\n' {
                           if cnt == 1 {
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
            .args(&[OsStr::new("-U"), OsStr::new("3"), old_p.as_os_str(), new_p.as_os_str()])
            .output().unwrap();
        let pos = skip_past_second_newline(&outp.stdout).unwrap_or(0);
        let diff_output_before = &outp.stdout[pos..];
        let mut diff_output : Vec<u8> = vec![];
        for line in diff_output_before.lines() {
            let line = line.unwrap();
            if line.starts_with("@@") && line.ends_with("@@") {
                diff_output.extend("--".bytes())
            } else {
                diff_output.extend(line.bytes())
            }
            diff_output.extend("\n".bytes())
        }
        let mut our_output : Vec<u8> = vec![];
        diff_files(&mut our_output, &old_p, &new_p).unwrap();
        if our_output != diff_output {
            eprintln!("outputs differ! ours:");
            io::stderr().write(&our_output).unwrap();
            eprintln!("diff's:");
            io::stderr().write(&diff_output).unwrap();
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
        let mut cnt = 0;
        for p in prod {
            eprintln!("Testing combo #{}", cnt);
            test_diff(&tmpdir, p.0, p.1);
            cnt += 1
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

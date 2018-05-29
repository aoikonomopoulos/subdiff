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

fn read_lines(p : &Path) -> io::Result<Vec<Vec<u8>>> {
    let f = File::open(p)?;
    let mut f = io::BufReader::new(f);
    let mut ret = vec![];
    loop {
        let mut buf = vec![];
        let len = f.read_until(b'\n', &mut buf)?;
        if len == 0 {
            return Ok (ret)
        }
        ret.push(buf)
    }
}

fn exist_differences(results : &[DiffResult<Vec<u8>>]) -> bool {
    results.iter().any(|r|
                       match r {
                           DiffResult::Common (_) => false,
                           _ => true,
                       })
}

fn diff_offsets<T : PartialEq + Clone>(d : &DiffResult<T>) -> (Option<usize>, Option<usize>) {
    match d {
        DiffResult::Added(el)
            | DiffResult::Removed(el)
            | DiffResult::Common(el) => (el.old_index, el.new_index)
    }
}

// The main difficulty in imitating diff's output is that the hunk header
// includes the length of the hunk, so we have to buffer our output and
// only print it out when we know the current hunk has ended. This saves
// the info we need in order to display the hunk header and related lines.
#[derive(Debug)]
struct Hunk {
    old_start : usize,
    old_len : usize,
    new_start : usize,
    new_len : usize,
    lines : Vec<DiffResult<Vec<u8>>>,
}

impl Hunk {
    // This is used when we have a change at the beginning of the file
    fn initial() -> Hunk {
        Hunk {
            old_start : 0,
            old_len : 0,
            new_start : 0,
            new_len : 0,
            lines : vec![]
        }
    }
    fn from_diff(d : DiffResult<Vec<u8>>) -> Hunk {
        match diff_offsets(&d) {
            (Some (o), Some (n)) => {
                Hunk {
                    old_start : o,
                    old_len : 1,
                    new_start : n,
                    new_len : 1,
                    lines : vec![d],
                }
            },
            _ => {
                panic!("Can currently ony start a hunk from a common element")
            },
        }
    }
    fn write(&self, old_lines : &[Vec<u8>], new_lines : &[Vec<u8>],
             out : &mut Write) -> io::Result<()> {
        writeln!(out, "@@ -{},{} +{},{} @@", self.old_start + 1, self.old_len,
                 self.new_start + 1, self.new_len)?;
        for d in &self.lines {
            match diff_offsets(d) {
                (Some (o), Some (_)) => {
                    out.write(b" ")?;
                    out.write(&old_lines[o][..])?;
                },
                (Some (o), None) => {
                    out.write(b"-")?;
                    out.write(&old_lines[o][..])?;
                },
                (None, Some (n)) => {
                    out.write(b"+")?;
                    out.write(&new_lines[n][..])?;
                },
                _ => panic!("Can't print DiffElement with neither side"),
            }
        };
        Ok (())
    }
    fn append(&mut self, d : DiffResult<Vec<u8>>) {
        match diff_offsets(&d){
            (Some (_), Some (_)) => { // Common
                self.old_len += 1;
                self.new_len += 1;
            },
            (Some (_), None) => { // Removal
                self.old_len += 1;
            },
            (None, Some (_)) => { // Addition
                self.new_len += 1;
            },
            _ => {
                panic!("DiffElement with neither side")
            },
        };
        self.lines.push(d)
    }
}

fn dump_hunk(out : &mut Write,
             old_lines : &[Vec<u8>],
             new_lines : &[Vec<u8>], hunk : Option<&Hunk>) -> io::Result<()> {
    eprintln!("In dump_hunk");
    match hunk {
        None => Ok (()),
        Some (hunk) => {
            hunk.write(old_lines, new_lines, out)
        }
    }
}

fn append(hunk : &mut Option<Hunk>, d : DiffResult<Vec<u8>>) {
    match hunk {
        None => {hunk.get_or_insert_with(|| Hunk::from_diff(d));},
        Some (h) => h.append(d),
    }
}

fn consume(hunk : &mut Option<Hunk>, ds : &mut Iterator<Item=DiffResult<Vec<u8>>>) {
    for d in ds {
        append(hunk, d)
    }
}

#[derive(Debug)]
enum State {
    // Customary diff behavior is to present any removes before immediately
    // following adds, however lcs_diff returns adds before removes. So we
    // set aside any consecutive adds and print them as soon as it's clear
    // we've observed (and emitted) all immediately following removes.
    CollectingAdds(Option<Hunk>, Vec<DiffResult<Vec<u8>>>),

    // Hold on to the last N common lines we've seen, dump them
    // as the preceeding context if a new change (addition/removal)
    // is seen.
    // We also need to prepend a separator if there were context
    // lines we had to drop, so our state also includes the number
    // of observed common lines while in this state.
    CollectingCommonsTail(Option<Hunk>, usize, VecDeque<DiffResult<Vec<u8>>>),

    // Accumulate up to $context lines, emit them, then switch
    // to CollectingCommonsTail.
    CollectingCommonsCorked(Option<Hunk>, VecDeque<DiffResult<Vec<u8>>>),

    // Emit seen remove, while holding on to any pending adds (see above)
    SequentialRemoves(Option<Hunk>, Vec<DiffResult<Vec<u8>>>),
}

fn display_diff_unified(out : &mut Write,
                        old_lines : &[Vec<u8>],
                        new_lines : &[Vec<u8>],
                        diff : Vec<DiffResult<Vec<u8>>>) -> io::Result<i32> {
    use State::*;
    if !exist_differences(&diff) {
        return Ok (0); // Exit w/o producing any output
    }
    let context = 3;

    let mut diff_results = diff.into_iter();
    // If the first diff result is an add or a remove, we need
    // to manually note down the start line in the hunk
    let mut state = match diff_results.next() {
        None => panic!("No differences at all, should have returned earlier"),
        Some (d) => {
            match d {
                DiffResult::Common(_) => {
                    let mut commons = VecDeque::new();
                    commons.push_back(d);
                    CollectingCommonsTail(None, 1, commons)
                },
                DiffResult::Added(_) => {
                    CollectingAdds(Some (Hunk::initial()), vec![d])
                },
                DiffResult::Removed(_) => {
                    let mut h = Hunk::initial();
                    h.append(d);
                    SequentialRemoves(Some (h), vec![])
                },
            }
        }
    };

    for d in diff_results {
        eprintln!("state = {:?}", state);
        eprintln!("processing diff result: {:?}", d);
        state = match state {
            CollectingAdds(mut hunk, mut adds) => {
                match d {
                    DiffResult::Added(_) => {
                        adds.push(d);
                        CollectingAdds(hunk, adds) // Still collecting adds
                    },
                    DiffResult::Removed(_) => {
                        append(&mut hunk, d);
                        // Change states, holding on to the pending adds
                        SequentialRemoves(hunk, adds)
                    },
                    DiffResult::Common(_) => {
                        // No adjacent removes, time to print out the adds
                        consume(&mut hunk, &mut adds.drain(..));
                        let mut commons = VecDeque::new();
                        commons.push_back(d);
                        // We've just seen a change; this needs to be followed by
                        // some context lines.
                        CollectingCommonsCorked(hunk, commons)
                    },
                }
            },
            CollectingCommonsTail(mut hunk, seen, mut commons) => {
                match d {
                    // If the state changes, print out the last N lines, possibly
                    // preceeded by a header
                    DiffResult::Added(_) => {
                        if seen > context {
                            dump_hunk(out, old_lines, new_lines, hunk.as_ref())?;
                            hunk = None
                        }
                        consume(&mut hunk, &mut commons.drain(..));
                        CollectingAdds(hunk, vec![d])
                    },
                    DiffResult::Removed(_) => {
                        if seen > context {
                            dump_hunk(out, old_lines, new_lines, hunk.as_ref())?;
                            hunk = None
                        }
                        consume(&mut hunk, &mut commons.drain(..));
                        append(&mut hunk, d);
                        SequentialRemoves(hunk, vec![])
                    },
                    DiffResult::Common(_) => {
                        commons.push_back(d);
                        if commons.len() > context {
                            commons.pop_front();
                        }
                        CollectingCommonsTail(hunk, seen + 1, commons)
                    },
                }
            },
            CollectingCommonsCorked(mut hunk, mut commons) => {
                match d {
                    // State change -> print collected common lines
                    DiffResult::Added(_) => {
                        consume(&mut hunk, &mut commons.drain(..));
                        CollectingAdds(hunk, vec![d])
                    },
                    DiffResult::Removed(_) => {
                        consume(&mut hunk, &mut commons.drain(..));
                        append(&mut hunk, d);
                        SequentialRemoves(hunk, vec![])
                    },
                    DiffResult::Common(_) => {
                        commons.push_back(d);
                        if commons.len() == context {
                            // We've accumulated $context common lines after
                            // a change; print out the hunk, then start collecting
                            // common lines to print _before_ the next change.
                            consume(&mut hunk, &mut commons.drain(..));
                            CollectingCommonsTail(hunk, 0, VecDeque::new())
                        } else {
                            CollectingCommonsCorked(hunk, commons)
                        }
                    },
                }
            },
            SequentialRemoves(mut hunk, mut adds) => {
                match d {
                    // State change -> time to print out the pending adds
                    DiffResult::Added(_) => {
                        consume(&mut hunk, &mut adds.drain(..));
                        CollectingAdds(hunk, vec![d])
                    },
                    DiffResult::Removed(_) => {
                        // Simply print out the remove
                        append(&mut hunk, d);
                        SequentialRemoves(hunk, adds)
                    },
                    DiffResult::Common(_) => {
                        consume(&mut hunk, &mut adds.drain(..));
                        let mut commons = VecDeque::new();
                        // XXX: handle context = 0
                        commons.push_back(d);
                        CollectingCommonsCorked(hunk, commons)
                    },
                }

            },
        }
    }
    eprintln!("Handling final state: {:?}", state);
    // Cleanup
    let hunk = match state {
        // We might end up here if the last additions are
        // exactly at the end of the file.
        CollectingAdds (mut hunk, mut adds) => {
            consume(&mut hunk, &mut adds.drain(..));
            hunk
        },
        // Those are common lines we collected in anticipation of the
        // next change. No change is coming any more, so drop them here.
        CollectingCommonsTail(mut hunk, _, _) => hunk,
        // We'll get here if there were < $context common lines between
        // the last change and the end of the file. We still need to
        // print them.
        CollectingCommonsCorked(mut hunk, mut commons) => {
            consume(&mut hunk, &mut commons.drain(..));
            hunk
        },
        // We may end up here if the last change is at the EOF.
        SequentialRemoves(mut hunk, mut adds) => {
            consume(&mut hunk, &mut adds.drain(..));
            hunk
        }
    };
    dump_hunk(out, old_lines, new_lines, hunk.as_ref())?;
    Ok (1)
}

fn diff_files(out : &mut Write, old : &Path, new : &Path) -> io::Result<i32> {
    let old_lines = read_lines(old)?;
    let new_lines = read_lines(new)?;

    let diff : Vec<DiffResult<Vec<u8>>> = lcs_diff::diff(&old_lines, &new_lines);
    display_diff_unified(out, &old_lines, &new_lines, diff)
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
        let diff_output = &outp.stdout[pos..];
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

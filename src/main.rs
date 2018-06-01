extern crate lcs_diff;
#[cfg(test)]
#[macro_use]
extern crate itertools;
#[cfg(test)]
extern crate temporary;
extern crate clap;
extern crate regex;

use self::lcs_diff::*;
use std::io;
use std::io::prelude::*;
use std::fs::File;
use std::fmt::Debug;
use std::path::Path;
use std::process::exit;
use std::str::FromStr;
use std::collections::VecDeque;
use clap::{App, Arg};
use regex::bytes::Regex;

struct Conf {
    debug : bool,
    context : usize,
    mark_changed_common: bool,
}

impl Conf {
    fn default() -> Conf {
        Conf {
            debug : false,
            context : 3,
            mark_changed_common : false,
        }
    }
}

macro_rules! dprintln {
    ($dbg:expr, $fmt:expr, $( $args:expr ),*) => {
        if cfg!(debug_assertions) {
            if $dbg {
                eprintln!($fmt, $( $args ),*)
            }
        }
    }
}

trait DisplayableHunk where Self::DiffItem : PartialEq + Clone + Debug + Sized {
    type DiffItem;
    fn do_write(&self, &Conf,
                &[Self::DiffItem], &[Self::DiffItem],
                &mut Write) -> io::Result<()>;
}

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

fn exist_differences<T : PartialEq + Clone>(results : &[DiffResult<T>]) -> bool {
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
struct Hunk<T : PartialEq + Clone> {
    old_start : usize,
    old_len : usize,
    new_start : usize,
    new_len : usize,
    lines : Vec<DiffResult<T>>,
}

impl<T: PartialEq + Clone> Hunk<T> {
    // This is used when we have a change at the beginning of the file
    fn initial() -> Hunk<T> {
        Hunk {
            old_start : 0,
            old_len : 0,
            new_start : 0,
            new_len : 0,
            lines : vec![]
        }
    }
    fn from_diff(d : DiffResult<T>) -> Hunk<T> {
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
    fn append(&mut self, d : DiffResult<T>) {
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

enum WordDiffState {
    ShowingCommon,
    ShowingRemoves,
    ShowingAdds,
}

fn extend(line : &mut Vec<u8>, s : &str) {
    line.extend(s.bytes())
}

impl DisplayableHunk for Hunk<u8> {
    type DiffItem = u8;
    fn do_write(&self, _ : &Conf, _ : &[u8], _ : &[u8], out : &mut Write) -> io::Result<()> {
        use WordDiffState::*;
        let mut line = vec![];
        let mut state = ShowingCommon;
        for d in &self.lines {
            state = match state {
                ShowingCommon => {
                    match d {
                        DiffResult::Common (el) => {
                            line.push(el.data);
                            ShowingCommon
                        },
                        DiffResult::Added (el) => {
                            extend(&mut line, "+{");
                            line.push(el.data);
                            ShowingAdds
                        },
                        DiffResult::Removed(el) => {
                            extend(&mut line, "-{");
                            line.push(el.data);
                            ShowingRemoves
                        }
                    }
                },
                ShowingAdds => {
                    match d {
                        DiffResult::Common (el) => {
                            line.push(b'}');
                            line.push(el.data);
                            ShowingCommon
                        },
                        DiffResult::Added (el) => {
                            line.push(el.data);
                            ShowingAdds
                        },
                        DiffResult::Removed (el) => {
                            line.push(b'}');
                            extend(&mut line, "-{");
                            line.push(el.data);
                            ShowingRemoves
                        },
                    }
                },
                ShowingRemoves => {
                    match d {
                        DiffResult::Common (el) => {
                            line.push(b'}');
                            line.push(el.data);
                            ShowingCommon
                        },
                        DiffResult::Added (el) => {
                            line.push(b'}');
                            extend(&mut line, "+{");
                            line.push(el.data);
                            ShowingAdds
                        },
                        DiffResult::Removed (el) => {
                            line.push(el.data);
                            ShowingRemoves
                        }
                    }
                },
            }
        }
        match state {
            ShowingAdds | ShowingRemoves => line.push(b'}'),
            ShowingCommon => (),
        };
        out.write(&line)?;
        Ok (())
    }
}

impl DisplayableHunk for Hunk<Vec<u8>> {
    type DiffItem = Vec<u8>;
    fn do_write(&self, conf : &Conf, old_lines : &[Vec<u8>], new_lines : &[Vec<u8>],
                out : &mut Write) -> io::Result<()> {
        writeln!(out, "@@ -{},{} +{},{} @@", self.old_start + 1, self.old_len,
                 self.new_start + 1, self.new_len)?;
        for d in &self.lines {
            match diff_offsets(d) {
                (Some (o), Some (n)) => {
                    let diff = lcs_diff::diff::<u8>(&old_lines[o][..], &new_lines[n][..]);
                    if !exist_differences(&diff) {
                        out.write(b" ")?;
                        out.write(&old_lines[o][..])?;
                    } else {
                        let pref = if conf.mark_changed_common {
                            b"="
                        } else {
                            b" "
                        };
                        out.write(pref)?;
                        let conf = Conf {context: 1000, ..*conf};
                        display_diff_unified::<u8>(out, &conf,
                                                   &old_lines[o][..],
                                                   &new_lines[n][..], diff)?;
                    }
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
}

fn append<T: PartialEq + Clone>(hunk : &mut Option<Hunk<T>>, d : DiffResult<T>) {
    match hunk {
        None => {hunk.get_or_insert_with(|| Hunk::from_diff(d));},
        Some (h) => h.append(d),
    }
}

fn consume<T : PartialEq + Clone>(hunk : &mut Option<Hunk<T>>,
                                  ds : &mut Iterator<Item=DiffResult<T>>) {
    for d in ds {
        append(hunk, d)
    }
}

#[derive(Debug)]
enum State<T : PartialEq + Clone + Debug> {
    // Customary diff behavior is to present any removes before immediately
    // following adds, however lcs_diff returns adds before removes. So we
    // set aside any consecutive adds and print them as soon as it's clear
    // we've observed (and emitted) all immediately following removes.
    CollectingAdds(Option<Hunk<T>>, Vec<DiffResult<T>>),

    // Hold on to the last N common lines we've seen, dump them
    // as the preceeding context if a new change (addition/removal)
    // is seen.
    // We also need to prepend a separator if there were context
    // lines we had to drop, so our state also includes the number
    // of observed common lines while in this state.
    CollectingCommonsTail(Option<Hunk<T>>, usize, VecDeque<DiffResult<T>>),

    // Accumulate up to $context lines, emit them, then switch
    // to CollectingCommonsTail.
    CollectingCommonsCorked(Option<Hunk<T>>, VecDeque<DiffResult<T>>),

    // Emit seen remove, while holding on to any pending adds (see above)
    SequentialRemoves(Option<Hunk<T>>, Vec<DiffResult<T>>),
}

fn display_diff_unified<T>(
    out : &mut Write,
                        conf : &Conf,
                        old_lines : &[T],
                        new_lines : &[T],
    diff : Vec<DiffResult<T>>) -> io::Result<i32>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use State::*;
    let mut dump_hunk = |hunk : Option<&Hunk<T>>| {
        match hunk {
            None => Ok (()),
            Some (hunk) => {
                hunk.do_write(conf, old_lines , new_lines, out)
            }
        }
    };
    let mut diff_results = diff.into_iter();
    // If the first diff result is an add or a remove, we need
    // to manually note down the start line in the hunk
    let mut state = match diff_results.next() {
        None => panic!("No differences at all, shouldn't have been called"),
        Some (d) => {
            match d {
                DiffResult::Common(_) => {
                    let mut commons = VecDeque::new();
                    commons.push_back(d);
                    CollectingCommonsTail(None, 1, commons)
                },
                DiffResult::Added(_) => {
                    CollectingAdds(Some (<Hunk<T>>::initial()), vec![d])
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
        dprintln!(conf.debug, "state = {:?}", state);
        dprintln!(conf.debug, "processing diff result: {:?}", d);
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
                        if seen > conf.context {
                            dump_hunk(hunk.as_ref())?;
                            hunk = None
                        }
                        consume(&mut hunk, &mut commons.drain(..));
                        CollectingAdds(hunk, vec![d])
                    },
                    DiffResult::Removed(_) => {
                        if seen > conf.context {
                            dump_hunk(hunk.as_ref())?;
                            hunk = None
                        }
                        consume(&mut hunk, &mut commons.drain(..));
                        append(&mut hunk, d);
                        SequentialRemoves(hunk, vec![])
                    },
                    DiffResult::Common(_) => {
                        commons.push_back(d);
                        if commons.len() > conf.context {
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
                        if commons.len() == conf.context {
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
    dprintln!(conf.debug, "Handling final state: {:?}", state);
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
    dump_hunk(hunk.as_ref())?;
    Ok (1)
}

fn extract_re_matches(conf : &Conf, re : &mut Regex, line : &[u8]) -> Vec<u8> {
    let mut ret = vec![];
    match re.captures(line) {
        Some (caps) => {
            for i in 1..caps.len() {
                let m = &caps[i];
                unsafe {
                    dprintln!(conf.debug, "Got match: `{:?}`",
                              String::from_utf8_unchecked(m.to_vec()))
                };
                ret.write(m).unwrap();
            }
        },
        None => {
            ret.write(line).unwrap();
        }
    }
    ret
}

fn pick_lines(conf : &Conf, mut re : &mut Regex, lines : &[Vec<u8>]) -> Vec<Vec<u8>> {
    lines.iter().map(|l| extract_re_matches(conf, &mut re, l)).collect()
}

fn diff_files(out : &mut Write, conf : &Conf, re : Option<&str>,
              old : &Path, new : &Path) -> io::Result<i32> {
    let old_lines = read_lines(old)?;
    let new_lines = read_lines(new)?;

    let diff : Vec<DiffResult<Vec<u8>>> = match re {
        Some (re) => {
            let mut re = match Regex::new(re) {
                Ok (re) => re,
                Err (err) => {
                    eprintln!("Could not compile regular expresssion `{}`: {}",
                              "XXX", err);
                    exit(2)
                }
            };
            lcs_diff::diff(&pick_lines(conf, &mut re, &old_lines),
                           &pick_lines(conf, &mut re, &new_lines))
        },
        None => lcs_diff::diff(&old_lines, &new_lines)
    };
    if !exist_differences(&diff) {
        return Ok (0); // Exit w/o producing any output
    }

    display_diff_unified::<Vec<u8>>(out, conf, &old_lines, &new_lines, diff)
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
        let conf = Conf {context: 3, ..Conf::default()};
        diff_files(&mut our_output, &conf, None, &old_p, &new_p).unwrap();
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

    fn do_wdiff(s1 : &str, s2 : &str, out : &mut Write) {
        let diff = lcs_diff::diff(s1.as_bytes(), s2.as_bytes());
        if exist_differences(&diff) {
            let conf = Conf {context : 1000, ..Conf::default()};
            display_diff_unified(out, &conf, s1.as_bytes(), s2.as_bytes(), diff);
        } else {
            out.write(s1.as_bytes());
        }
    }

    fn check_wdiff(s1 : &str, s2 : &str, exp : &str) {
        let mut out : Vec<u8> = vec![];
        do_wdiff(s1, s2, &mut out);
        if &out[..] != exp.as_bytes() {
            eprintln!("old: `{}`", s1);
            eprintln!("new: `{}`", s2);
            eprintln!("Expected: `{}`", exp);
            io::stderr().write(b"Got: `").unwrap();
            io::stderr().write(&out).unwrap();
            eprintln!("`");
            panic!("Incorrect wdiff output");
        }
    }

    #[test]
    fn test_wdiff() {
        check_wdiff("", "", "");
        check_wdiff("", "a", "+{a}");
        check_wdiff("a", "", "-{a}");
        check_wdiff("a", "a", "a");

        check_wdiff("ab", "ab", "ab");
        check_wdiff("ac", "abc", "a+{b}c");
        check_wdiff("abc", "ac", "a-{b}c");

        check_wdiff("ad", "abcd", "a+{bc}d");
        check_wdiff("abcd", "ad", "a-{bc}d");
        check_wdiff("ac", "abcd", "a+{b}c+{d}");
        check_wdiff("acd", "abc", "a+{b}c-{d}");
        check_wdiff("abc", "adc", "a-{b}+{d}c");
        check_wdiff("abcd", "aefd", "a-{bc}+{ef}d");
    }
}

fn parse_usize(s : &str) -> usize {
    match usize::from_str(s) {
        Ok (u) => u,
        Err (e) => {
            eprintln!("Error parsing '{}' as usize: {}", s, e);
            exit(2)
        }
    }
}

fn main() {
    let matches = App::new("subdiff")
        .version("0.1")
        .arg(Arg::with_name("context")
             .short("c")
             .long("context")
             .help("Number of displayed context lines")
             .default_value("3"))
        .arg(Arg::with_name("old")
             .required(true)
             .index(1)
             .help("OLD file"))
        .arg(Arg::with_name("new")
             .required(true)
             .index(2)
             .help("NEW file"))
        .arg(Arg::with_name("common_re")
             .required(false)
             .short("r")
             .long("regex")
             .takes_value(true)
             .value_name("RE")
             .help("Compare the parts of lines matched by this regexp"))
        .get_matches();

    let context = parse_usize(matches.value_of("context").unwrap());
    let conf = Conf {
        context : context,
        ..Conf::default()
    };
    let ecode = match diff_files(&mut io::stdout(),
                                 &conf,
                                 matches.value_of("common_re"),
                                 Path::new(matches.value_of("old").unwrap()),
                                 Path::new(matches.value_of("new").unwrap())) {
        Ok (ecode) => ecode,
        Err (err) => {
            eprintln!("Error comparing files: {}", err);
            2
        },
    };
    exit(ecode);
}

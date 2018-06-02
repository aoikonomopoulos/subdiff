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
use std::path::Path;
use std::process::exit;
use std::str::FromStr;
use clap::{App, Arg};
use regex::bytes::Regex;

macro_rules! dprintln {
    ($dbg:expr, $fmt:expr, $( $args:expr ),*) => {
        if cfg!(debug_assertions) {
            if $dbg {
                eprintln!($fmt, $( $args ),*)
            }
        }
    }
}

pub mod conf;
pub mod hunked;
pub mod wdiff;

use conf::*;
use hunked::*;

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

    display_diff_hunked::<Vec<u8>>(out, conf, &old_lines, &new_lines, diff)
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
            // Testing is deterministic, this helps with being
            // able to tell if a failing test is now succeeding
            dprintln!(false, "Testing combo #{}", cnt);
            test_diff(&tmpdir, p.0, p.1);
            cnt += 1
        }
        tmpdir.remove().unwrap()
    }

    fn do_wdiff(s1 : &str, s2 : &str, out : &mut Write) {
        let diff = lcs_diff::diff(s1.as_bytes(), s2.as_bytes());
        if exist_differences(&diff) {
            let conf = Conf {context : 1000, ..Conf::default()};
            display_diff_hunked(out, &conf, s1.as_bytes(), s2.as_bytes(), diff).unwrap();
        } else {
            out.write(s1.as_bytes()).unwrap();
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

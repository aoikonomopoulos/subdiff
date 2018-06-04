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
use clap::{App, Arg, Values};
use regex::bytes::{Regex, RegexSet, RegexBuilder, RegexSetBuilder};

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

#[cfg(test)]
pub mod tests;

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
    results.iter().any(|r| match r {
        DiffResult::Common (_) => false,
        _ => true,
    })
}

fn sel_part_of_line(conf : &Conf, re : &Regex, line : &[u8]) -> Option<Vec<u8>> {
    if let Some (caps) = re.captures(line) {
        let mut ret = vec![];
        for i in 1..caps.len() {
            match caps.get(i) {
                Some (m) => {
                    dprintln!(conf.debug, "Got match[{}]: `{}`", i,
                              String::from_utf8(m.as_bytes().to_vec()).unwrap());
                    ret.write_all(m.as_bytes()).unwrap()
                },
                None => {
                    dprintln!(conf.debug, "No match[{}]", i)
                }
            }
        }
        Some (ret)
    } else {
        None
    }
}

trait ReSelector {
    fn sel(&self, &Conf, &[u8]) -> Option<Vec<u8>>;
}

struct SingleRe(Regex);

impl SingleRe {
    fn build(s : &str) -> SingleRe {
        // Note: Our lines contain the EOL character. Use multi-line mode, so that
        // $ can match the EOL and the RE will still work if the user does ^foo$.
        match RegexBuilder::new(s).multi_line(true).build() {
            Ok (re) => SingleRe(re),
            Err (err) => {
                eprintln!("Could not compile regular expression `{}`: {}",
                          s, err);
                exit(2)
            }
        }
    }
}

impl ReSelector for SingleRe {
    fn sel(&self, conf : &Conf, line : &[u8]) -> Option<Vec<u8>> {
        sel_part_of_line(conf, &self.0, line)
    }
}

struct MultiRe {
    multi : RegexSet,
    regexes : Vec<Regex>,
}

impl MultiRe {
    fn build<I, S>(strs : I) -> MultiRe
    where S : AsRef<str>,
    I : IntoIterator<Item = S> + Clone
    {
        // Compile the individual REs first, so that we can tell
        // the user which RE had an error.
        let regexes =
            strs.clone().into_iter().map(|s| {
                match RegexBuilder::new(s.as_ref()).multi_line(true).build() {
                    Ok (re) => re,
                    Err (err) => {
                        eprintln!("Could not compile regular expression `{}`: {}",
                                  s.as_ref(), err);
                        exit(2)
                    },
                }
            }).collect();
        let multi = match RegexSetBuilder::new(strs).multi_line(true).build() {
            Ok (set) => set,
            Err (err) => {
                eprintln!("Could not build regular expression set: {}", err);
                exit(2)
            },
        };
        MultiRe {
            multi,
            regexes,
        }
    }
}

impl ReSelector for MultiRe {
    fn sel(&self, conf : &Conf, line : &[u8]) -> Option<Vec<u8>> {
        let mut matches = self.multi.matches(line).into_iter();
        match matches.next() {
            None => None,
            Some (single) => {
                match matches.next() {
                    None => {
                        let re = &self.regexes[single];
                        match sel_part_of_line(conf, re, line) {
                            m @ Some (_) => m,
                            None => panic!("RegexSet claimed a match, but the RE disagrees")
                        }
                    },
                    Some (_) => {
                        eprintln!("Line is matched by more than \
                                   one regular expression:");
                        io::stderr().write_all(b"`").unwrap();
                        io::stderr().write_all(line).unwrap();
                        eprintln!("` is matched by:");
                        for re in &self.regexes {
                            eprintln!("{}", re);
                        }
                        exit(2)
                    }
                }
            }
        }
    }
}

fn build_re_selector<I, S>(re_strs : I) -> Box<ReSelector>
where
    S : AsRef<str>,
    I : IntoIterator<Item=S> + Clone
{
    let len = re_strs.clone().into_iter().count();
    // When the user specified a single RE, don't use a RegexSet,
    // so that we can get the matches w/o running it twice.
    // When we are given >1 RE, we need to scan all REs anyway, in
    // order to make sure there's exactly one match. In that case,
    // use RegexSet to scan in parallel, then go back and run only
    // the RE that matched to determine what parts of the line to
    // use.
    match len {
        1 => {
            let s = re_strs.into_iter().next().unwrap();
            Box::new(SingleRe::build(s.as_ref()))
        },
        _ => Box::new(MultiRe::build(re_strs)),
    }
}

fn extract_re_matches(conf : &Conf, re : &ReSelector, line : &[u8]) -> Vec<u8> {
    match re.sel(conf, line) {
        None => line.to_vec(),
        Some (s) => s,
    }
}

fn pick_lines(conf : &Conf, mre : &ReSelector, lines : &[Vec<u8>]) -> Vec<Vec<u8>> {
    lines.iter().map(|l| extract_re_matches(conf, mre, l)).collect()
}

fn diff_files(out : &mut Write, conf : &Conf, re : Option<Values>,
              old : &Path, new : &Path) -> io::Result<i32> {
    let old_lines = read_lines(old)?;
    let new_lines = read_lines(new)?;

    let diff : Vec<DiffResult<Vec<u8>>> = match re {
        Some (values) => {
            let mre = build_re_selector(values);
            lcs_diff::diff(&pick_lines(conf, &*mre, &old_lines),
                           &pick_lines(conf, &*mre, &new_lines))
        },
        None => lcs_diff::diff(&old_lines, &new_lines)
    };
    if !exist_differences(&diff) {
        return Ok (0); // Exit w/o producing any output
    }

    display_diff_hunked::<Vec<u8>>(out, conf, &old_lines, &new_lines, diff)
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
             .multiple(true)
             .number_of_values(1)
             .value_name("RE")
             .help("Compare the parts of lines matched by this regexp"))
        .get_matches();

    let context = parse_usize(matches.value_of("context").unwrap());
    let conf = Conf {
        context,
        ..Conf::default()
    };
    let ecode = match diff_files(&mut io::stdout(),
                                 &conf,
                                 matches.values_of("common_re"),
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

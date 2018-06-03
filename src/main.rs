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
use regex::bytes::{Regex, RegexSet};

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
    results.iter().any(|r|
                       match r {
                           DiffResult::Common (_) => false,
                           _ => true,
                       })
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
            strs.clone().into_iter().map(|s| match Regex::new(s.as_ref()) {
                Ok (re) => re,
                Err (err) => {
                    eprintln!("Could not compile regular expression `{}`: {}",
                              s.as_ref(), err);
                    exit(2)
                },
            }).collect();
        let multi = match RegexSet::new(strs) {
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
    fn sub(&self, conf : &Conf, line : &[u8]) -> Option<Vec<u8>> {
        let mut matches = self.multi.matches(line).into_iter();
        match matches.next() {
            None => None,
            Some (single) => {
                match matches.next() {
                    None => {
                        let mut ret = vec![];
                        let re = &self.regexes[single];
                        if let Some (caps) = re.captures(line) {
                            for i in 1..caps.len() {
                                match caps.get(i) {
                                    Some (m) => {
                                        dprintln!(conf.debug, "Got match[{}]: `{}`", i,
                                                  String::from_utf8(m.as_bytes().to_vec()).unwrap());
                                        ret.write_all(m.as_bytes()).unwrap();
                                    },
                                    None => {
                                        dprintln!(conf.debug, "No match[{}]", i);
                                    }
                                }
                            }
                        } else {
                            panic!("RegexSet claimed a match, but the RE disagrees")
                        }
                        Some (ret)
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

fn extract_re_matches(conf : &Conf, re : &mut MultiRe, line : &[u8]) -> Vec<u8> {
    match re.sub(conf, line) {
        None => line.to_vec(),
        Some (s) => s,
    }
}

fn pick_lines(conf : &Conf, mut mre : &mut MultiRe, lines : &[Vec<u8>]) -> Vec<Vec<u8>> {
    lines.iter().map(|l| { extract_re_matches(conf, &mut mre, l)
    }).collect()
}

fn diff_files(out : &mut Write, conf : &Conf, re : Option<Values>,
              old : &Path, new : &Path) -> io::Result<i32> {
    let old_lines = read_lines(old)?;
    let new_lines = read_lines(new)?;

    let diff : Vec<DiffResult<Vec<u8>>> = match re {
        Some (values) => {
            let mut mre = MultiRe::build(values);
            lcs_diff::diff(&pick_lines(conf, &mut mre, &old_lines),
                           &pick_lines(conf, &mut mre, &new_lines))
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

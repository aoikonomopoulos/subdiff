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

fn extract_re_matches(conf : &Conf, re : &mut Regex, line : &[u8]) -> Vec<u8> {
    let mut ret = vec![];
    match re.captures(line) {
        Some (caps) => {
            for i in 1..caps.len() {
                let m = &caps[i];
                dprintln!(conf.debug, "Got match: `{}`",
                          String::from_utf8(m.to_vec()).unwrap());
                ret.write_all(m).unwrap();
            }
        },
        None => {
            ret.write_all(line).unwrap();
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
        context,
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

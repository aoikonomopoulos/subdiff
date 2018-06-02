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

use super::*;
use itertools::Itertools;
use std::process::Command;
use std::ffi::OsStr;
use std::usize;
use conf::ContextLineFormat::*;
use conf::CharacterClassExpansion::*;
use conf::ContextLineTokenization::*;

enum TestDiff {
    AgainstDiff,
    AgainstGiven (Vec<u8>),
}

fn diff_two_files(conf : &Conf, old : &Path, new : &Path) -> Vec<u8> {
    let outp = Command::new("diff")
        .args(&[OsStr::new("-U"),
                OsStr::new(&conf.context.to_string()),
                old.as_os_str(),
                new.as_os_str()])
        .output().unwrap();
    outp.stdout
}

fn test_diff<'a, I>(conf : &Conf, dir : &temporary::Directory, test: TestDiff,
                    res : Option<I>, ignore_re : Option<&str>,
                    lines1 : &[&str], lines2 : &[&str])
where
    I : IntoIterator<Item = &'a str> + Clone
{
    let old_p = dir.join("old");
    let mut old = File::create(&old_p).unwrap();
    let new_p = dir.join("new");
    let mut new = File::create(&new_p).unwrap();

    for l in lines1 {
        write!(&mut old, "{}", l).unwrap();
    }
    old.flush().unwrap();
    for l in lines2 {
        write!(&mut new, "{}", l).unwrap();
    }
    new.flush().unwrap();
    let mut our_output : Vec<u8> = vec![];
    diff_files(&mut our_output, conf, res, ignore_re, &old_p, &new_p).unwrap();
    let expected = match test {
        TestDiff::AgainstDiff => diff_two_files(conf, &old_p, &new_p),
        TestDiff::AgainstGiven (s) => {
            let mut complete = vec![];
            file_header(&mut complete, b"---", &old_p).unwrap();
            file_header(&mut complete, b"+++", &new_p).unwrap();
            complete.extend(s);
            complete
        },
    };
    if our_output != expected {
        eprintln!("outputs differ! ours:");
        io::stderr().write(&our_output).unwrap();
        eprintln!("expected:");
        io::stderr().write(&expected).unwrap();
        panic!("Output differs to the expected bytes")
    }
}

fn do_chunk(conf : &Conf, idx : usize, chunk : &[(usize, usize)], lines : &[&str]) {
    let tmpdir = temporary::Directory::new("diff-test").unwrap();
    let mut cnt = 0;
    for (i, j) in chunk {
        let combos1 : Vec<Vec<&str>> = lines.iter().cloned().combinations(*i).collect();
        let combos2 : Vec<Vec<&str>> = lines.iter().cloned().combinations(*j).collect();
        let prod = iproduct!(combos1, combos2);
        for p in prod {
            // Testing is deterministic, this helps with being
            // able to tell if a failing test is now succeeding
            dprintln!(conf.debug, "Testing combo ({}, {})", idx, cnt);
            let no_res : Option<Vec<&'static str>> = None;
            test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res, None, &p.0, &p.1);
            cnt += 1
        }
    }
    tmpdir.remove().unwrap()
}

#[test]
fn long_test_of_combos_against_diff() {
    let conf = Conf {
        context: 1,
        debug : false,
        ..Conf::default()
    };
    let lines : Vec<&str>
        = vec!["a\n", "b\n", "c\n", "d\n", "e\n", "f\n", "g\n", "h\n", "i\n", "j\n"];

    let mut cnt = 0;
    let prod : Vec<(usize, usize)> = iproduct!(0..6, 0..6).collect();
    rayon::scope(|s| {
        for chunk in prod.chunks(2) {
            let lines = &lines;
            for context in 0..2 {
                let conf = Conf {context, ..conf};
                s.spawn(move |_| {
                    do_chunk(&conf, cnt, chunk, lines)
                });
                cnt += 1
            }
        }
    });
}

fn do_wdiff(s1 : &str, s2 : &str, out : &mut Write) {
    let diff = lcs_diff::diff(s1.as_bytes(), s2.as_bytes());
    if exist_differences(&diff) {
        let conf = Conf {
            context : usize::MAX,
            context_tokenization : Char,
            ..Conf::default()
        };
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
fn context_wdiff() {
    check_wdiff("", "", "");
    check_wdiff("", "a", "{+a}");
    check_wdiff("a", "", "{-a}");
    check_wdiff("a", "a", "a");

    check_wdiff("ab", "ab", "ab");
    check_wdiff("ac", "abc", "a{+b}c");
    check_wdiff("abc", "ac", "a{-b}c");

    check_wdiff("ad", "abcd", "a{+bc}d");
    check_wdiff("abcd", "ad", "a{-bc}d");
    check_wdiff("ac", "abcd", "a{+b}c{+d}");
    check_wdiff("acd", "abc", "a{+b}c{-d}");
    check_wdiff("abc", "adc", "a{-b}{+d}c");
    check_wdiff("abcd", "aefd", "a{-bc}{+ef}d");
}

fn do_newline_at_eof(conf : &Conf) {
    let tmpdir = temporary::Directory::new("newline-at-eof").unwrap();
    let no_res : Option<Vec<&'static str>> = None;
    // Both files end w/o a newline.
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n", "b"], &["a\n", "b"]);

    // Both files end at a newline.
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n", "b\n"], &["a\n", "b\n"]);

    // Old file ends w/o a newline.
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n", "b"], &["a\n", "b\n"]);

    // New file ends w/o a newline.
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n", "b\n"], &["a\n", "b"]);

    // Test earlier printing of no-newline message.
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n", "b\n", "c\n", "d\n", "e\n", "f"],
              &["a\n"]);
    test_diff(&conf, &tmpdir, TestDiff::AgainstDiff, no_res.clone(), None,
              &["a\n"],
              &["a\n", "b\n", "c\n", "d\n", "e\n", "f"]);
    tmpdir.remove().unwrap()
}

#[test]
fn newline_at_eof_handling() {
    for context in 0..2 {
        let conf = Conf {
            debug : false,
            context,
            ..Conf::default()
        };
        do_newline_at_eof(&conf)
    }
}

fn join_lines(lines : Vec<&str>) -> Vec<u8> {
    lines.into_iter().fold(vec![], |mut acc, el| {
        acc.extend(el.bytes());
        acc.push(b'\n');
        acc
    })
}

fn test_given<'a, I>(conf : &Conf, res : Option<I>, ignore_re : Option<&str>,
              old : &[&str], new : &[&str], expected : Vec<u8>)
where
    I : IntoIterator<Item = &'a str> + Clone
{
    let tmpdir = temporary::Directory::new("sel-smoke-test").unwrap();
    test_diff(&conf, &tmpdir, TestDiff::AgainstGiven(expected),
              res, ignore_re, old, new)
}

#[test]
fn single_re_works() {
    let conf = Conf {
        debug : false,
        context : 1,
        ..Conf::default()
    };
    let re = Some (vec![r"^(\w+)\s+\w+\s+\w+$"]);
    let expected = join_lines(vec![
        "@@ -2,2 +2,2 @@",
        " d e f",
        "-g h i",
        "+x h i"
    ]);
    test_given(&conf, re, None,
               &["a b c\n", "d e f\n", "g h i\n"],
               &["a x c\n", "d e f\n", "x h i\n"],
               expected)
}

#[test]
fn multiple_res_work() {
    let conf = Conf {
        debug : false,
        context : 1,
        context_format : ContextLineFormat::Wdiff,
        context_tokenization : Char,
        ..Conf::default()
    };
    let re = Some (vec![
        // only last word matches, if line starts with a letter
        r"^[a-z]+\s+\w+\s+(\w+)$",
        // only last word matches, if line starts with a digit
        r"^\d+\s+\w+\s+(\w+)$",
    ]);
    let expected = join_lines(vec![
        "@@ -1,6 +1,6 @@",
        " a {-b}{+x} c",
        "-d e f",
        "+d e x",
        " {-1}{+2} g h",
        "-1 i j",
        "+1 i x",
        " k l m",
        "-& o p", // Unmatched line should appear as a difference
        "+& x p",
    ]);
    test_given(&conf, re, None,
               &["a b c\n", "d e f\n", "1 g h\n", "1 i j\n", "k l m\n", "& o p\n"],
               &["a x c\n", "d e x\n", "2 g h\n", "1 i x\n", "k l m\n", "& x p\n"],
               expected)
}

#[test]
fn ignore_re_works() {
    let conf = Conf {
        debug : false,
        context : 1,
        context_tokenization : Char,
        ..Conf::default()
    };
    let re : Option<Vec<&'static str>> = None;
    let ignore_re = Some (r"\b\d\b|0x[a-f0-9]+");
    let expected = join_lines(vec![
        "@@ -2,2 +2,2 @@",
        " d e 0x{-f00}{+eac}",
        "-g h i",
        "+x h i"
    ]);
    test_given(&conf, re, ignore_re,
               &["a 1 c\n", "d e 0xf00\n", "g h i\n"],
               &["a 2 c\n", "d e 0xeac\n", "x h i\n"],
               expected)
}

#[test]
fn re_and_ignore_re_work() {
    let conf = Conf {
        debug : false,
        context : 1,
        context_tokenization : Char,
        ..Conf::default()
    };
    let re = Some (vec![
        // only last word matches, if line starts with a letter
        r"^[a-z]+\s+\w+\s+(\w+)$",
        // only last word matches, if line starts with a digit
        r"^\d+\s+\w+\s+(\w+)$",
    ]);

    let ignore_re = Some (r"\b\d\b|0x[a-f0-9]+");
    let expected = join_lines(vec![
        // Change at 1st line selected-out by 1st RE
        "@@ -2,5 +2,5 @@",
        // Change at 2nd line ignored by ignore_re, appears as common
        " d e 0x{-f00}{+eac}",
        // Matched by 2nd RE
        "-1 i j",
        "+1 i x",
        // Matched by 2nd RE but digits ignored by ignore_re
        " 1 l {-2}{+3}",
        // Not matched by the REs, but digits ignored by ignore_re
        " & {-3}{+4} o",
        // Not matched by the REs and nothing ignored
        "-& p q",
        "+# p q",
    ]);
    test_given(&conf, re, ignore_re,
               &["a b c\n", "d e 0xf00\n", "1 i j\n", "1 l 2\n", "& 3 o\n", "& p q\n"],
               &["a x c\n", "d e 0xeac\n", "1 i x\n", "1 l 3\n", "& 4 o\n", "# p q\n"],
               expected)
}

#[test]
fn character_class_wide() {
    let conf = Conf {
        debug : false,
        context : 100,
        context_format : CC (Wide),
        context_tokenization : Char,
        ..Conf::default()
    };
    let re = Some (vec![
        // only last word matches, if line starts with a letter
        r"^[a-z]+\s+\S+\s+(\w+)$",
    ]);
    let expected = join_lines(vec![
        r"@@ -1,6 +1,6 @@",
        r" a z\a{1}w c",
        r"-1 e f",
        r"+1 e x",
        r" g 0\d{1,2}9 h",
        r" j z\w{2}w k",
        r" n z.{3}w o",
        // Test that multiple non-adjacent changes are printed properly.
        r" p a\a{1}c\a{1}ef q",
    ]);
    test_given(&conf, re, None,
               &["a zbw c\n", "1 e f\n", "g 019 h\n",  "j zl2w k\n",
                 "n zp1@w o\n", "p abcdef q\n"],
               &["a zxw c\n", "1 e x\n", "g 0229 h\n", "j zm3w k\n",
                 "n zx24w o\n", "p axcyef q\n"],
               expected)
}

#[test]
fn character_class_narrow() {
    let conf = Conf {
        debug : false,
        context : 100,
        context_format : CC (Narrow),
        context_tokenization : Char,
        ..Conf::default()
    };
    let re = Some (vec![
        // only last word matches, if line starts with a letter
        r"^[a-z]+\s+\S+\s+(\w+)$",
    ]);
    let expected = join_lines(vec![
        r"@@ -1,8 +1,8 @@",
        r" aa \a+ c",
        r"-1 e f",
        r"+1 e x",
        r" g \d+ h",
        r" j \w+ k",
        r" n z.+w o",
        r" p \a+ r",
        // Test summarization of multiple, non-adjacent, changes.
        r" \a+ \d+ u",
        // Test that a\ac\aef is collapsed to \a, not \a\a.
        r" \a+ \d+ w",
    ]);
    test_given(&conf, re, None,
               &["aa zbw c\n", "1 e f\n", "g 019 h\n",  "j zl2w k\n", "n zp1@w o\n", "p abcdef r\n",
                 "jun 12 u\n", "abcdef 7 w\n"],
               &["aa zxw c\n", "1 e x\n", "g 0229 h\n", "j zm3w k\n", "n zx24w o\n", "p aBCDEf r\n",
                 "nov 09 u\n", "axcyef 8 w\n"],
               expected)
}

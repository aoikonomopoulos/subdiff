use std::io;
use std::io::prelude::*;
use std::iter::Peekable;
use itertools::Itertools;
use super::lcs_diff::*;
use hunked::Hunk;
use conf::{Conf, CharacterClassExpansion};

#[cfg_attr(feature = "cargo-clippy", allow(enum_variant_names))]
enum WordDiffState {
    ShowingCommon,
    ShowingRemoves,
    ShowingAdds,
}

fn extend(line : &mut Vec<u8>, s : &str) {
    line.extend(s.bytes())
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum CharacterClass {
    White,
    Digit,
    Alpha,
    Word,
    Any,
}

impl CharacterClass {
    fn init(ch : u8) -> CharacterClass {
        use self::CharacterClass::*;
        if ch.is_ascii_alphabetic() {
            Alpha
        } else if ch.is_ascii_digit() {
            Digit
        } else if ch.is_ascii_whitespace() {
            White
        } else {
            Any
        }
    }
    fn accepts(&self, ch : u8) -> bool {
        use self::CharacterClass::*;
        match self {
            White => ch.is_ascii_whitespace(),
            Digit => ch.is_ascii_digit(),
            Alpha => ch.is_ascii_alphabetic(),
            Word => ch.is_ascii_alphabetic() || ch.is_ascii_digit(),
            Any => unreachable!(),
        }
    }
    fn add(&mut self, ch : u8) -> CharacterClass {
        use self::CharacterClass::*;
        if ch.is_ascii_alphabetic() {
            match self {
                Digit => Word,
                Alpha => Alpha,
                Word => Word,
                _ => Any,
            }
        } else if ch.is_ascii_digit() {
            match self {
                Digit => Digit,
                Alpha | Word => Word,
                _ => Any,
            }
        } else if ch.is_ascii_whitespace() {
            match self {
                White => White,
                _ => Any,
            }
        } else {
            Any
        }
    }
    fn write(&self, out : &mut Write) -> io::Result<()>{
        use self::CharacterClass::*;
        match self {
            White => out.write_all(b"\\s"),
            Digit => out.write_all(b"\\d"),
            Alpha => out.write_all(b"\\a"),
            Word => out.write_all(b"\\w"),
            Any => out.write_all(b".")
        }
    }
}

fn res_data<T : PartialEq + Clone>(res : &DiffResult<T>) -> T {
    match res {
        DiffResult::Common (el) => el.data.clone(),
        DiffResult::Added (el) => el.data.clone(),
        DiffResult::Removed (el) => el.data.clone(),
    }
}

fn is_common(d : &DiffResult<u8>) -> bool {
    match d {
        DiffResult::Common (_) => true,
        _ => false,
    }
}

fn narrow_do_common<'a, I>(out : &mut Write,
                         prev_cc : Option<CharacterClass>,
                         mut acc : Vec<u8>,
                         mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<u8>>,
    Peekable<I> : Clone,
{
    // Accumulate common characters in anticipation of a change.
    {
        let common = items.peeking_take_while(|d| is_common(d)).map(res_data);
        acc.extend(common);
    }
    narrow_do_differences(out, prev_cc, acc, items)
}

fn skip_common<'a, I>(cc : CharacterClass, items : &mut Peekable<I>)
where
    I : Iterator<Item=&'a DiffResult<u8>>,
{
    items.peeking_take_while(|d| {
        match d {
            DiffResult::Common (_) => {
                match cc {
                    CharacterClass::Any => false,
                    cc => cc.accepts(res_data(d)),
                }
            },
            _ => false,
        }
    }).count();
}

fn narrow_do_differences<'a, I>(out : &mut Write,
                             prev_cc : Option<CharacterClass>,
                             context_pre : Vec<u8>,
                             mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<u8>>,
    Peekable<I> : Clone,
{
    let first = items.next();
    let first = match first {
        None => {
            // End of line, print out the accumulated context.
            out.write_all(&context_pre)?;
            return Ok (())
        },
        Some (d) => {
            assert!(!is_common(d));
            d
        }
    };
    // There is at least one change, we're in business.
    let cc = CharacterClass::init(res_data(first));

    // Go over the changes to determine the CC.
    let cc = items.peeking_take_while(|d| !is_common(d))
        .map(res_data).fold(cc, |mut cc : CharacterClass, ch| cc.add(ch));

    // See if any adjacent _common_ characters to our left can be
    // included in the current character class.
    let n_unsummarizable = context_pre.iter().rev().skip_while(|ch| {
        match cc {
            CharacterClass::Any => false,
            cc => cc.accepts(**ch),
        }
    }).count();
    // Output the common characters to our left that are not
    // summarized by the current CC.
    out.write_all(&context_pre[..n_unsummarizable])?;

    // If the previously printed CC was the same as the current one
    // AND we were able to include all preceeding common characters
    // in the current CC, skip printing the CC (or we would produce
    // things like \w\w).
    let print_cc = match prev_cc {
        Some (prev_cc) if prev_cc == cc => n_unsummarizable != 0,
        _ => true
    };
    if print_cc {
        cc.write(out)?;
        out.write_all(b"+")?;
    }

    // Omit any adjacent common characters to our right that
    // are compatible with our current CC.
    skip_common(cc, &mut items);
    narrow_do_common(out, Some (cc), vec![], items)
}

fn wide_do_differences<'a, I>(out : &mut Write,
                     mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<u8>>,
    Peekable<I> : Clone,
{
    let mut nadded = 0;
    let mut nremoved = 0;
    let cc = {
        let mut count = |d : &DiffResult<u8>| {
            match d {
                // We only get called by minimal_do_common, which should
                // have consumed all commons, and for elements of the
                // !is_common iterator below.
                DiffResult::Common (_) => unreachable!(),
                DiffResult::Added (_) => nadded += 1,
                DiffResult::Removed (_) => nremoved += 1,
            }
        };
        let cc = match items.next() {
            None => return Ok (()),
            Some (d) => {
                count(d);
                CharacterClass::init(res_data(d))
        },
        };
        items.peeking_take_while(|d| !is_common(d))
            .fold(cc, |mut cc : CharacterClass, d| {
                count(d);
                cc.add(res_data(d))
            })
    };
    cc.write(out)?;
    if nadded == nremoved {
        write!(out, "{{{}}}", nadded)?;
    } else {
        write!(out, "{{{},{}}}", nremoved, nadded)?;
    }
    wide_do_common(out, items)
}

fn wide_do_common<'a, I>(out : &mut Write,
                     mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<u8>>,
    Peekable<I> : Clone,
{
    let common : Vec<u8> = items.peeking_take_while(|d| is_common(d)).map(res_data).collect();
    out.write_all(&common)?;
    wide_do_differences(out, items)
}

pub fn intra_line_write_cc(hunk : &Hunk<u8>,
                           expansion : CharacterClassExpansion,
                           _ : &Conf, _ : &[u8], _ : &[u8],
                           out : &mut Write) -> io::Result<()> {
    use CharacterClassExpansion::*;
    let items = hunk.items.iter().peekable();
    match expansion {
        Narrow => {
            narrow_do_common(out, None, vec![], items)
        },
        Wide => {
            wide_do_common(out, items)
        }
    }
}

pub fn intra_line_write_wdiff(hunk : &Hunk<u8>, _ : &Conf,
                          _ : &[u8], _ : &[u8],
                          out : &mut Write) -> io::Result<()> {
    use self::WordDiffState::*;
    let mut line = vec![];
    let mut state = ShowingCommon;
    for d in &hunk.items {
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
    out.write_all(&line)?;
    Ok (())
}

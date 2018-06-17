use std::io;
use std::io::prelude::*;
use std::iter::Peekable;
use std::marker::PhantomData;
use itertools::Itertools;
use super::lcs_diff::*;
use super::Regex;
use hunked::Hunk;
use conf::{Conf, CharacterClassExpansion};

#[derive(PartialEq, Clone, Debug)]
pub struct Word(Vec<u8>); // XXX: &[u8]

pub trait Writeable {
    // This basically means: "can serialize itself into bytes". Can
    // hopefully switch to std::slice::from_ref (needed for u8) once
    // it becomes stable.
    fn write_to(&self, out : &mut Write) -> io::Result<()>;
}

impl Writeable for u8 {
    fn write_to(&self, out : &mut Write) -> io::Result<()> {
        let mut w = vec![];
        w.push(*self);
        out.write_all(&w[..])
    }
}

impl Writeable for Word {
    fn write_to(&self, out : &mut Write) -> io::Result<()> {
        out.write_all(&self.0[..])
    }
}

impl<'a, T> Writeable for &'a [T]
where
    T: Writeable,
{
    // We rely on our callers to give us a buffer, not a File,
    // or performance will suffer.
    fn write_to(&self, out : &mut Write) -> io::Result<()> {
        for w in self.iter() {
            w.write_to(out).unwrap()
        }
        Ok (())
    }
}

impl<T> Writeable for Vec<T>
where
    T : Writeable,
{
    fn write_to(&self, out : &mut Write) -> io::Result<()> {
        (&self[..]).write_to(out)
    }
}

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
pub enum CharacterClass<T : PartialEq + Clone> {
    White,
    Digit,
    Alpha,
    Word,
    Any (PhantomData<T>),
}

impl Into<CharacterClass<Word>> for CharacterClass<u8> {
    fn into(self) -> CharacterClass<Word> {
        use self::CharacterClass::*;
        match self {
            White => White,
            Digit => Digit,
            Alpha => Alpha,
            Word => Word,
            Any (_) => Any (PhantomData),
        }
    }
}

pub trait HasCharacterClass {
    type Item : Clone + PartialEq;
    fn cc(&self) -> CharacterClass<Self::Item>;
}

impl HasCharacterClass for u8 {
    type Item = u8;
    fn cc(&self) -> CharacterClass<u8> {
        use self::CharacterClass::*;
        if self.is_ascii_alphabetic() {
            Alpha
        } else if self.is_ascii_digit() {
            Digit
        } else if self.is_ascii_whitespace() {
            White
        } else {
            Any (PhantomData)
        }
    }
}

impl HasCharacterClass for Word {
    type Item = Word;
    fn cc(&self) -> CharacterClass<Word> {
        let mut chars = self.0.iter();
        let cc = match chars.next() {
            None => CharacterClass::Any (PhantomData),
            Some (ch) => ch.cc()
        };
        let cc : CharacterClass<u8> = chars.fold(cc, |cc, ch| cc.merge(&ch.cc()));
        cc.into()
    }
}

impl<T> CharacterClass<T>
where
    T: PartialEq + Clone + HasCharacterClass<Item=T>
{
    fn merge(&self, other : &Self) -> Self {
        use self::CharacterClass::*;
        match other {
            White => {
                match self {
                    White => White,
                    _ => Any (PhantomData),
                }
            },
            Digit => {
                match self {
                    Digit => Digit,
                    Alpha | Word => Word,
                    _ => Any (PhantomData)
                }
            },
            Alpha => {
                match self {
                    Alpha => Alpha,
                    Digit | Word => Word,
                    _ => Any (PhantomData),
                }
            },
            Word => {
                match self {
                    Alpha | Digit | Word => Word,
                    _ => Any (PhantomData),
                }
            },
            _ => Any (PhantomData),
        }
    }
    fn accepts(&self, el : &T) -> bool {
        let ncc = self.merge(&el.cc());
        &ncc == self
    }
    fn write(&self, out : &mut Write) -> io::Result<()>{
        use self::CharacterClass::*;
        match self {
            White => out.write_all(b"\\s"),
            Digit => out.write_all(b"\\d"),
            Alpha => out.write_all(b"\\a"),
            Word => out.write_all(b"\\w"),
            Any (_) => out.write_all(b".")
        }
    }
}

fn res_data<T : PartialEq + Clone + HasCharacterClass>(res : &DiffResult<T>) -> T {
    match res {
        DiffResult::Common (el) => el.data.clone(),
        DiffResult::Added (el) => el.data.clone(),
        DiffResult::Removed (el) => el.data.clone(),
    }
}

fn is_common<T : PartialEq + Clone>(d : &DiffResult<T>) -> bool {
    match d {
        DiffResult::Common (_) => true,
        _ => false,
    }
}

fn narrow_do_common<'a, I, T>(out : &mut Write,
                         prev_cc : Option<CharacterClass<T>>,
                         mut acc : Vec<T>,
                         mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<T>>,
    T: PartialEq + Clone + HasCharacterClass<Item=T> + Writeable + 'a,
    Peekable<I> : Clone,
{
    // Accumulate common characters in anticipation of a change.
    {
        let common = items.peeking_take_while(|d| is_common(d)).map(res_data);
        acc.extend(common);
    }
    narrow_do_differences(out, prev_cc, acc, items)
}

fn skip_common<'a, T, I>(cc : &CharacterClass<T>, items : &mut Peekable<I>)
where
    I : Iterator<Item=&'a DiffResult<T>>,
    T: PartialEq + Clone + HasCharacterClass<Item=T> + 'a,
{
    items.peeking_take_while(|d| {
        match d {
            DiffResult::Common (_) => {
                match cc {
                    CharacterClass::Any (_) => false,
                    ref cc => cc.accepts(&res_data(d)),
                }
            },
            _ => false,
        }
    }).count();
}

fn narrow_do_differences<'a, T, I>(out : &mut Write,
                             prev_cc : Option<CharacterClass<T>>,
                             context_pre : Vec<T>,
                             mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<T>>,
    T: PartialEq + Clone + HasCharacterClass<Item=T> + Writeable + 'a,
    Peekable<I> : Clone,
{
    let first = items.next();
    let first = match first {
        None => {
            // End of line, print out the accumulated context.
            context_pre.write_to(out)?;
            return Ok (())
        },
        Some (d) => {
            assert!(!is_common(d));
            d
        }
    };
    // There is at least one change, we're in business.
    let cc = res_data(first).cc();

    // Go over the changes to determine the CC.
    let cc = items.peeking_take_while(|d| !is_common(d))
        .map(res_data).fold(cc, |cc : CharacterClass<T>, ch| cc.merge(&ch.cc()));

    // See if any adjacent _common_ characters to our left can be
    // included in the current character class.
    let n_unsummarizable = context_pre.iter().rev().skip_while(|ch| {
        match cc {
            CharacterClass::Any (_) => false,
            ref cc => cc.accepts(*ch),
        }
    }).count();
    // Output the common characters to our left that are not
    // summarized by the current CC.
    (&context_pre[..n_unsummarizable]).write_to(out)?;

    // If the previously printed CC was the same as the current one
    // AND we were able to include all preceeding common characters
    // in the current CC, skip printing the CC (or we would produce
    // things like \w\w).
    let print_cc = match prev_cc {
        Some (ref prev_cc) if prev_cc == &cc => n_unsummarizable != 0,
        _ => true
    };
    if print_cc {
        cc.write(out)?;
        out.write_all(b"+")?;
    }

    // Omit any adjacent common characters to our right that
    // are compatible with our current CC.
    skip_common(&cc, &mut items);
    narrow_do_common(out, Some (cc), vec![], items)
}

fn wide_do_differences<'a, T, I>(out : &mut Write,
                     mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<T>>,
    T: PartialEq + Clone + HasCharacterClass<Item=T> + Writeable + 'a,
    Peekable<I> : Clone,
{
    let mut nadded = 0;
    let mut nremoved = 0;
    let cc = {
        let mut count = |d : &DiffResult<T>| {
            match d {
                // We only get called by wide_do_common, which should
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
                res_data(d).cc()
        },
        };
        items.peeking_take_while(|d| !is_common(d))
            .fold(cc, |cc : CharacterClass<T>, d| {
                count(d);
                cc.merge(&res_data(d).cc())
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

fn wide_do_common<'a, T, I>(out : &mut Write,
                     mut items : Peekable<I>) -> io::Result<()>
where
    I : Iterator<Item=&'a DiffResult<T>>,
    T: PartialEq + Clone + HasCharacterClass<Item=T> + Writeable + 'a,
    Peekable<I> : Clone,
{
    let common : Vec<T> = items.peeking_take_while(|d| is_common(d)).map(res_data).collect();
    common.write_to(out)?;
    wide_do_differences(out, items)
}

pub fn intra_line_write_cc<T>(hunk : &Hunk<T>,
                              expansion : CharacterClassExpansion,
                              _ : &Conf, _ : &[T], _ : &[T],
                              out : &mut Write) -> io::Result<()>
where
    T: PartialEq + Clone + HasCharacterClass<Item=T> + Writeable,
{
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

pub fn intra_line_write_wdiff<T>(hunk : &Hunk<T>, _ : &Conf,
                                 _ : &[T], _ : &[T],
                                 out : &mut Write) -> io::Result<()>
where
    T: PartialEq + Clone + Writeable,
{
    use self::WordDiffState::*;
    let mut line : Vec<u8> = vec![];
    let mut state = ShowingCommon;
    for d in &hunk.items {
        state = match state {
            ShowingCommon => {
                match d {
                    DiffResult::Common (el) => {
                        el.data.write_to(&mut line)?;
                        ShowingCommon
                    },
                    DiffResult::Added (el) => {
                        extend(&mut line, "+{");
                        el.data.write_to(&mut line)?;
                        ShowingAdds
                    },
                    DiffResult::Removed(el) => {
                        extend(&mut line, "-{");
                        el.data.write_to(&mut line)?;
                        ShowingRemoves
                    }
                }
            },
            ShowingAdds => {
                match d {
                    DiffResult::Common (el) => {
                        line.push(b'}');
                        el.data.write_to(&mut line)?;
                        ShowingCommon
                    },
                    DiffResult::Added (el) => {
                        el.data.write_to(&mut line)?;
                        ShowingAdds
                    },
                    DiffResult::Removed (el) => {
                        line.push(b'}');
                        extend(&mut line, "-{");
                        el.data.write_to(&mut line)?;
                        ShowingRemoves
                    },
                }
            },
            ShowingRemoves => {
                match d {
                    DiffResult::Common (el) => {
                        line.push(b'}');
                        el.data.write_to(&mut line)?;
                        ShowingCommon
                    },
                    DiffResult::Added (el) => {
                        line.push(b'}');
                        extend(&mut line, "+{");
                        el.data.write_to(&mut line)?;
                        ShowingAdds
                    },
                    DiffResult::Removed (el) => {
                        el.data.write_to(&mut line)?;
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

pub fn tokenize(line : &[u8]) -> Vec<Word> {
    let re = Regex::new(r"\b").unwrap();
    re.split(line).map(|w| Word(w.to_vec())).collect()
}

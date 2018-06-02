use std::io;
use std::io::prelude::*;
use super::lcs_diff::*;
use hunked::Hunk;
use conf::Conf;

enum WordDiffState {
    ShowingCommon,
    ShowingRemoves,
    ShowingAdds,
}

fn extend(line : &mut Vec<u8>, s : &str) {
    line.extend(s.bytes())
}

#[derive(Debug)]
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
    fn add(&mut self, ch : u8) -> CharacterClass {
        use self::CharacterClass::*;
        if ch.is_ascii_alphabetic() {
            match self {
                Digit => Word,
                Alpha => Alpha,
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
            White => out.write_all("\\s".as_bytes()),
            Digit => out.write_all("\\d".as_bytes()),
            Alpha => out.write_all("\\a".as_bytes()),
            Word => out.write_all("\\w".as_bytes()),
            Any => out.write_all(".".as_bytes())
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

pub fn intra_line_write_cc(hunk : &Hunk<u8>, _ : &Conf, _ : &[u8], _ : &[u8], out : &mut Write) -> io::Result<()> {
    let mut cc = None;
    let mut nadded = 0;
    let mut nremoved = 0;
    for d in &hunk.items {
        let ch = match d {
            DiffResult::Common (_) => None,
            DiffResult::Added (el) => {
                nadded += 1;
                Some (el.data)
            },
            DiffResult::Removed (el) => {
                nremoved += 1;
                Some (el.data)
            },
        };
        cc = match ch {
            None => cc,
            Some (ch) => {
                match cc {
                    None => Some (CharacterClass::init(ch)),
                    Some (mut cc) => Some (cc.add(ch)),
                }
            },
        };
    }
    let chars = hunk.items.iter();
    let context_pre : Vec<u8> = chars
        .take_while(|d| match d {
            DiffResult::Common (_) => true,
            _ => false,
        }).map(res_data).collect();
    out.write_all(&context_pre)?;

    for cc in cc.iter() {
        cc.write(out)?;
    }
    if nadded == nremoved {
        write!(out, "{{{}}}", nadded)?;
    } else {
        write!(out, "{{{},{}}}", nremoved, nadded)?;
    }

    let context_post : Vec<u8> =
        hunk.items.iter().skip(context_pre.len())
        .skip_while(|d| match d {
            DiffResult::Common (_) => false,
            _ => true,
        }).map(res_data).collect();
    out.write_all(&context_post)
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
    out.write(&line)?;
    Ok (())
}

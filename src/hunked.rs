use std::io;
use std::io::prelude::*;
use std::fmt::Debug;
use std::collections::VecDeque;
use super::lcs_diff;
use super::lcs_diff::{DiffResult, DiffElement};
use super::conf::{Conf, ContextLineFormat};
use super::wdiff::*;

pub trait DisplayableHunk where Self::DiffItem : PartialEq + Clone + Debug + Sized {
    type DiffItem;
    fn do_write(&self, &Conf,
                &[Self::DiffItem], &[Self::DiffItem],
                &mut Write) -> io::Result<()>;
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
pub struct Hunk<T : PartialEq + Clone> {
    old_start : usize,
    old_len : usize,
    new_start : usize,
    new_len : usize,
    pub items : Vec<DiffResult<T>>,
}

impl<T: PartialEq + Clone> Hunk<T> {
    // This is used when we have a change at the beginning of the file
    fn initial() -> Hunk<T> {
        Hunk {
            old_start : 0,
            old_len : 0,
            new_start : 0,
            new_len : 0,
            items : vec![]
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
                    items : vec![d],
                }
            },
            _ => {
                panic!("Can currently ony start a hunk from a common element")
            },
        }
    }
    fn append(&mut self, d : DiffResult<T>) {
        match d {
            DiffResult::Common (_) => {
                self.old_len += 1;
                self.new_len += 1;
            },
            DiffResult::Removed (_) => {
                self.old_len += 1;
            },
            DiffResult::Added (_) => {
                self.new_len += 1;
            },
        };
        self.items.push(d)
    }
}

impl DisplayableHunk for Hunk<u8> {
    type DiffItem = u8;
    fn do_write(&self, conf : &Conf,
                o : &[u8], n : &[u8],
                out : &mut Write) -> io::Result<()> {
        match conf.context_format {
            ContextLineFormat::CC =>
                intra_line_write_cc(&self, conf, o, n, out),
            ContextLineFormat::Wdiff =>
                intra_line_write_wdiff(&self, conf, o, n, out),
            ContextLineFormat::Old =>
                out.write_all(o),
            ContextLineFormat::New =>
                out.write_all(n),
        }
    }
}

fn write_off_len(out : &mut Write,
                 lines : &[Vec<u8>],
                 off : usize, len : usize) -> io::Result<()> {
    // Special case galore: if the file is empty, its (offset, len) in
    // the hunk header has to be 0,0.
    if off == 0 && lines.is_empty() {
        write!(out, "0,0")?;
        return Ok (())
    } else {
        write!(out, "{}", off + 1)?;
    }
    // If the length of the lines in the hunk for this file is 1,
    // diff doesn't include the length in the output.
    if len > 1 {
        write!(out, ",{}", len)?;
    }
    Ok (())
}

fn write_hunk_header(out : &mut Write,
                     old_lines : &[Vec<u8>], new_lines : &[Vec<u8>],
                     hunk : &Hunk<Vec<u8>>) -> io::Result<()> {
    let mut header = vec![];
    write!(header, "@@ -")?;
    write_off_len(&mut header, old_lines, hunk.old_start, hunk.old_len)?;
    write!(header, " +")?;
    write_off_len(&mut header, new_lines, hunk.new_start, hunk.new_len)?;
    writeln!(header, " @@")?;
    out.write_all(&header)
}

impl DisplayableHunk for Hunk<Vec<u8>> {
    type DiffItem = Vec<u8>;
    fn do_write(&self, conf : &Conf, old_lines : &[Vec<u8>], new_lines : &[Vec<u8>],
                out : &mut Write) -> io::Result<()> {
        write_hunk_header(out, old_lines, new_lines, self)?;
        let mut last_removed_nl = None;
        let mut last_added_nl = None;
        for d in self.items.iter().rev() {
            match d {
                DiffResult::Common (_) => (),
                DiffResult::Removed (r) => {
                    match last_removed_nl {
                        Some (_) => (),
                        None => {
                            let o = r.old_index.unwrap();
                            if o < (old_lines.len() - 1) {
                                continue
                            }
                            let lo = &old_lines[o][..];
                            last_removed_nl = Some (lo[lo.len() - 1] == b'\n');
                            if last_added_nl.is_some() {
                                break
                            }
                        }
                    }
                },
                DiffResult::Added (a) => {
                    match last_added_nl {
                        Some (_) => (),
                        None => {
                            let n = a.new_index.unwrap();
                            if n < (new_lines.len() - 1) {
                                continue
                            }
                            let ln = &new_lines[n][..];
                            last_added_nl = Some (ln[ln.len() - 1] == b'\n');
                            if last_removed_nl.is_some() {
                                break
                            }
                        }
                    }
                },
            }
        }
        for d in &self.items {
            match d {
                DiffResult::Common (DiffElement { old_index : Some (o), new_index : Some (n), ..}) => {
//                (Some (o), Some (n)) => {
                    let diff = lcs_diff::diff::<u8>(&old_lines[*o][..], &new_lines[*n][..]);
                    if !super::exist_differences(&diff) {
                        out.write_all(b" ")?;
                        out.write_all(&old_lines[*o][..])?;
                    } else {
                        let pref = if conf.mark_changed_context {
                            b"!"
                        } else {
                            b" "
                        };
                        out.write_all(pref)?;
                        let conf = Conf {context: 1000, ..conf.clone()};
                        display_diff_hunked::<u8>(out, &conf,
                                                   &old_lines[*o][..],
                                                   &new_lines[*n][..], diff)?;
                    }
                },
                DiffResult::Removed (DiffElement { old_index : Some (o), ..}) => {
//                (Some (o), None) => {
                    out.write_all(b"-")?;
                    out.write_all(&old_lines[*o][..])?;
                    if *o == (old_lines.len() - 1) {
                        match (last_removed_nl, last_added_nl) {
                            (Some (o_has_nl), Some (n_has_nl)) => {
                                if !o_has_nl && n_has_nl {
                                    out.write_all(b"\n\\ No newline at end of file\n")?;
                                }
                            },
                            (Some (false), None) => {
                                out.write_all(b"\n\\ No newline at end of file\n")?;
                            },
                            _ => (),
                        }
                    }
                },
                DiffResult::Added (DiffElement { new_index : Some (n), ..}) => {
//                (None, Some (n)) => {
                    out.write_all(b"+")?;
                    out.write_all(&new_lines[*n][..])?;
                    if *n == (new_lines.len() - 1) {
                        match (last_removed_nl, last_added_nl) {
                            (Some (o_has_nl), Some (n_has_nl)) => {
                                if o_has_nl && !n_has_nl {
                                    out.write_all(b"\n\\ No newline at end of file\n")?;
                                }
                            },
                            (None, Some (false)) => {
                                out.write_all(b"\n\\ No newline at end of file\n")?;
                            },
                            _ => (),
                        }
                    }
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

    // Hold on to the last N common items we've seen, dump them
    // as the preceeding context if a new change (addition/removal)
    // is seen.
    // We also need to prepend a separator if there were context
    // items we had to drop, so our state also includes the number
    // of observed common items while in this state.
    CollectingCommonsTail(Option<Hunk<T>>, usize, VecDeque<DiffResult<T>>),

    // Accumulate up to $context items, emit them, then switch
    // to CollectingCommonsTail.
    CollectingCommonsCorked(Option<Hunk<T>>, VecDeque<DiffResult<T>>),

    // Emit seen remove, while holding on to any pending adds (see above)
    SequentialRemoves(Option<Hunk<T>>, Vec<DiffResult<T>>),
}

// struct FSM<T>
// where T : PartialEq + Clone + Debug + 't,
// Hunk<T> : DisplayableHunk<DiffItem=T> {
//     setup : &'static Fn(Option<DiffResult<T>>) -> State<T>,
//     fsm : &'static Fn(&Conf, &mut FnMut(Option<&Hunk<T>>) -> io::Result<()>,
//              State<T>, DiffResult<T>) -> io::Result<State<T>>,
//     handle_final : &'static Fn(&Conf, &mut FnMut(Option<&Hunk<T>>) -> io::Result<()>,
//                       State<T>) -> io::Result<()>,
// }

fn setup_initial_state<T>(diff : Option<DiffResult<T>>) -> State<T>
where T : PartialEq + Clone + Debug,
    Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
    match diff {
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
    }
}

fn handle_final_state<T>(conf : &Conf,
                         dump_hunk : &mut FnMut(Option<&Hunk<T>>) ->
                         io::Result<()>,
                         state : State<T>) -> io::Result<()>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
    dprintln!(conf.debug, "Handling final state: {:?}", state);
    // Cleanup
    let hunk = match state {
        // We might end up here if the last additions are
        // exactly at the end of the file.
        CollectingAdds (mut hunk, mut adds) => {
            consume(&mut hunk, &mut adds.drain(..));
            hunk
        },
        // Those are common items we collected in anticipation of the
        // next change. No change is coming any more, so drop them here.
        CollectingCommonsTail(mut hunk, _, _) => hunk,
        // We'll get here if there were < $context common items between
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
    dump_hunk(hunk.as_ref())
}

fn fsm<T>(conf : &Conf,
          dump_hunk : &mut FnMut(Option<&Hunk<T>>) -> io::Result<()>,
          state : State<T>, d : DiffResult<T>) -> io::Result<State<T>>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
    let state = match state {
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
                    // some context items.
                    CollectingCommonsCorked(hunk, commons)
                },
            }
        },
        CollectingCommonsTail(mut hunk, seen, mut commons) => {
            match d {
                // If the state changes, print out the last N items, possibly
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
                    if seen > conf.context {
                        dump_hunk(hunk.as_ref())?;
                        hunk = None
                    }
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
                // State change -> print collected common items
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
                    if commons.len() == conf.context {
                        // We've accumulated $context common items after
                        // a change; print out the hunk, then start collecting
                        // common items to print _before_ the next change.
                        consume(&mut hunk, &mut commons.drain(..));
                        commons.push_back(d);
                        CollectingCommonsTail(hunk, 1, commons)
                    } else {
                        commons.push_back(d);
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
    };
    Ok (state)
}

fn setup_initial_state_nocontext<T>(diff : Option<DiffResult<T>>) -> State<T>
where T : PartialEq + Clone + Debug,
    Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
    match diff {
        None => panic!("No differences at all, shouldn't have been called"),
        Some (d) => {
            match d {
                DiffResult::Common(_) => {
                    let mut h = Hunk::initial();
                    SequentialRemoves(Some (h), vec![])
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
    }
}

fn fsm_nocontext<T>(_conf : &Conf,
          dump_hunk : &mut FnMut(Option<&Hunk<T>>) -> io::Result<()>,
          state : State<T>, d : DiffResult<T>) -> io::Result<State<T>>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
    let state = match state {
        CollectingAdds(mut hunk, mut adds) => {
            match d {
                DiffResult::Added(_) => {
                    adds.push(d);
                    CollectingAdds(hunk, adds)
                },
                DiffResult::Removed(_) => {
                    append(&mut hunk, d);
                    SequentialRemoves(hunk, adds)
                },
                DiffResult::Common(_) => {
                    consume(&mut hunk, &mut adds.drain(..));
                    dump_hunk(hunk.as_ref())?;
                    SequentialRemoves(None, vec![])
                },
            }
        },
        SequentialRemoves(mut hunk, mut adds) => {
            match d {
                DiffResult::Added(_) => {
                    consume(&mut hunk, &mut adds.drain(..));
                    CollectingAdds(hunk, vec![d])
                },
                DiffResult::Removed(_) => {
                    append(&mut hunk, d);
                    SequentialRemoves(hunk, adds)
                },
                DiffResult::Common(_) => {
                    consume(&mut hunk, &mut adds.drain(..));
                    dump_hunk(hunk.as_ref())?;
                    SequentialRemoves(None, vec![])
                },
            }
        },
        CollectingCommonsCorked (_, _) | CollectingCommonsTail (_, _, _) => {
            panic!("Got CollectingCommons* in no-context")
        }
    };
    Ok (state)
}

pub fn display_diff_hunked<T>(
    out : &mut Write,
    conf : &Conf,
    old_lines : &[T],
    new_lines : &[T],
    diff : Vec<DiffResult<T>>) -> io::Result<i32>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
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
    let mut state = if conf.context > 0 {
        setup_initial_state(diff_results.next())
    } else {
        setup_initial_state_nocontext(diff_results.next())
    };

    for d in diff_results {
        dprintln!(conf.debug, "state = {:?}", state);
        dprintln!(conf.debug, "processing diff result: {:?}", d);
        state = if conf.context > 0 {
            fsm(conf, &mut dump_hunk, state, d)?
        } else {
            fsm_nocontext(conf, &mut dump_hunk, state, d)?
        };
    }
    handle_final_state(conf, &mut dump_hunk, state)?;
    Ok (1)
}

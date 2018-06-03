use std::io;
use std::io::prelude::*;
use std::fmt::Debug;
use std::collections::VecDeque;
use super::lcs_diff;
use super::lcs_diff::DiffResult;
use super::conf::Conf;
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
        match diff_offsets(&d){
            (Some (_), Some (_)) => { // Common
                self.old_len += 1;
                self.new_len += 1;
            },
            (Some (_), None) => { // Removal
                self.old_len += 1;
            },
            (None, Some (_)) => { // Addition
                self.new_len += 1;
            },
            _ => {
                panic!("DiffElement with neither side")
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
        if conf.summarize_common_cc {
            intra_line_write_cc(&self, conf, o, n, out)
        } else {
            intra_line_write_wdiff(&self, conf, o, n, out)
        }
    }
}

impl DisplayableHunk for Hunk<Vec<u8>> {
    type DiffItem = Vec<u8>;
    fn do_write(&self, conf : &Conf, old_lines : &[Vec<u8>], new_lines : &[Vec<u8>],
                out : &mut Write) -> io::Result<()> {
        writeln!(out, "@@ -{},{} +{},{} @@", self.old_start + 1, self.old_len,
                 self.new_start + 1, self.new_len)?;
        for d in &self.items {
            match diff_offsets(d) {
                (Some (o), Some (n)) => {
                    let diff = lcs_diff::diff::<u8>(&old_lines[o][..], &new_lines[n][..]);
                    if !super::exist_differences(&diff) {
                        out.write_all(b" ")?;
                        out.write_all(&old_lines[o][..])?;
                    } else {
                        let pref = if conf.mark_changed_common {
                            b"="
                        } else {
                            b" "
                        };
                        out.write_all(pref)?;
                        let conf = Conf {context: 1000, ..*conf};
                        display_diff_hunked::<u8>(out, &conf,
                                                   &old_lines[o][..],
                                                   &new_lines[n][..], diff)?;
                    }
                },
                (Some (o), None) => {
                    out.write_all(b"-")?;
                    out.write_all(&old_lines[o][..])?;
                },
                (None, Some (n)) => {
                    out.write_all(b"+")?;
                    out.write_all(&new_lines[n][..])?;
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

pub fn display_diff_hunked<T>(
    out : &mut Write,
    conf : &Conf,
    old_lines : &[T],
    new_lines : &[T],
    diff : Vec<DiffResult<T>>) -> io::Result<i32>
where T : PartialEq + Clone + Debug,
Hunk<T> : DisplayableHunk<DiffItem=T>
{
    use self::State::*;
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
    let mut state = match diff_results.next() {
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
    };

    for d in diff_results {
        dprintln!(conf.debug, "state = {:?}", state);
        dprintln!(conf.debug, "processing diff result: {:?}", d);
        state = match state {
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
                        commons.push_back(d);
                        if commons.len() == conf.context {
                            // We've accumulated $context common items after
                            // a change; print out the hunk, then start collecting
                            // common items to print _before_ the next change.
                            consume(&mut hunk, &mut commons.drain(..));
                            CollectingCommonsTail(hunk, 0, VecDeque::new())
                        } else {
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
        }
    }
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
    dump_hunk(hunk.as_ref())?;
    Ok (1)
}

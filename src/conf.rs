
#[derive(Clone, Copy, PartialEq)]
pub enum CharacterClassExpansion {
    Narrow,
    Wide,
}

#[derive(Clone, Copy)]
pub enum ContextLineFormat {
    CC (CharacterClassExpansion),
    Wdiff,
    Old,
    New,
}

impl ContextLineFormat {
    pub fn allowed_values() -> Vec<&'static str> {
        vec!["cc", "ccwide", "wdiff", "old", "new"]
    }
    pub fn new(s : &str) -> ContextLineFormat {
        use self::ContextLineFormat::*;
        use self::CharacterClassExpansion::*;
        if s == "cc" {
            CC (Narrow)
        } else if s == "ccmin" {
            CC (Wide)
        } else if s == "wdiff" {
            Wdiff
        } else if s == "old" {
            Old
        } else if s == "new" {
            New
        } else {
            panic!("Unsupported value: `{}`", s);
        }
    }
}

#[derive(Clone)]
pub struct Conf {
    pub debug : bool,
    pub context : usize,
    pub mark_changed_context: bool,
    pub context_format: ContextLineFormat,
    pub display_selected: bool,
}

impl Conf {
    pub fn default() -> Conf {
        Conf {
            debug : false,
            context : 3,
            mark_changed_context : false,
            context_format : ContextLineFormat::Wdiff,
            display_selected : false,
        }
    }
}

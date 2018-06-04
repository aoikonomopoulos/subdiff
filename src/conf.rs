#[derive(Clone)]
pub enum ContextLineFormat {
    CC,
    Wdiff,
    Old,
}

impl ContextLineFormat {
    pub fn allowed_values() -> Vec<&'static str> {
        vec!["cc", "wdiff", "old"]
    }
    pub fn from_str(s : &str) -> ContextLineFormat {
        use self::ContextLineFormat::*;
        if s == "cc" {
            CC
        } else if s == "wdiff" {
            Wdiff
        } else if s == "old" {
            Old
        } else {
            panic!("Unsupported value: `{}`", s);
        }
    }
}

#[derive(Clone)]
pub struct Conf {
    pub debug : bool,
    pub context : usize,
    pub mark_changed_common: bool,
    pub context_format: ContextLineFormat,
}

impl Conf {
    pub fn default() -> Conf {
        Conf {
            debug : false,
            context : 3,
            mark_changed_common : false,
            context_format : ContextLineFormat::Wdiff,
        }
    }
}

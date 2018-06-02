pub struct Conf {
    pub debug : bool,
    pub context : usize,
    pub mark_changed_common: bool,
    pub summarize_common_cc: bool,
}

impl Conf {
    pub fn default() -> Conf {
        Conf {
            debug : false,
            context : 3,
            mark_changed_common : false,
            summarize_common_cc : false,
        }
    }
}

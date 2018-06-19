% SUBDIFF(1) User Manual
% Angelos Oikonomopoulos
% June 2018

# NAME

subdiff - substring diff program

# SYNOPSIS

subdiff [*options*] *old-file* *new-file*

# DESCRIPTION

`subdiff` is entirely analogous to `diff`, except it can be asked to only
compare parts of the line selected by a regular expression, but still
output the original input lines after comparison. Its output format
faithfully follows that of `diff`, so that its output can be passed to
tools like `diffstat` and `colordiff`.

# OPTIONS

-r *RE*, \--regex=RE
:   Specify regular expression. For lines which are matched by RE,
    only the parts of the line which are matched by top-level capture
    groups take part in the comparison. A line that is not matched by
    a regular expression is compared in whole.

    This option can be given multiple times. It is the responsibility
    of the user to ensure that no input line is matched by more than
    one regular expression -- a runtime error is generated otherwise.

    Multiple regular expressions are searched for in parallel (i.e
    they are compiled into the same automaton); if a single regular
    expression matches, it is then re-run by itself in order to build
    up the capture groups.

-i *RE*, \--ignore=RE
:   Specify character sequences that should be *ignored*. The provided
    RE is only considered as a whole (i.e. individual subgroups are
    not looked at). This is run after any regular expressions
    specified by `-r` and can match multiple times per line; any of
    its matches are removed from the selected substring.

    This option makes it easier to ignore variable parts of the input
    that can also appear a variable number of times per line. For
    example, it exclude from comparison anything that looks like a
    hexadecimal address. It can also be used to ignore changes
    in the amount of whitespace.

    As this RE is matched as a whole, this option can only be given a
    single time. To ignore more than one regular expression, the user
    should specify them as alternatives, i.e. `"RE1|RE2"`.

-c N, \--context=N
:   Number of context lines to be displayed

\--context-format=CTXFMT
:   Display format for any displayed context lines.

    Context lines do not, by definition, have differences in the parts
    of the line that were selected by a regular expression. That said,
    they can still have differences in the parts that were not
    selected (or ignored). This option specifies the display style to
    use for such differences.

    * *wdiff* Word-diff style. This is the default. It presents
       changes in the context lines using wdiff-style
       `{-removed}{+added}` markers.

    * *ccwide* Summarize by character class. Whenever all the
        characters in consecutive changes can be said to belong to one
        of the predefined character classes, do not print out the
        changes themselves; instead, output the character class
        followed by the count of the characters in the old and new
        version.

        Character classes are printed out as either \\c{n}, when
        both the old and new versions consist of `n` characters of
        this class, or \\c{o,n} when the old and new versions consist
        of `o` and `n` number of characters respectively.
	The character classes are
	     + \\a alphabetic
	     + \\d digit
	     + \\w word (i.e. digit or alphabetic)
	     + \\s whitespace
	     + . any
    * *cc* Aggressively summarize by character class. This functions
        like *ccwide* above, but will also pull in any adjacent
        characters that are common between the two files (therefore
        the output will be more "narrow", i.e. fewer
        characters). Given that we are marking changes to lines that
        the user specifically ignored, this is the most appropriate
        option to summarize changes to values where *wdiff* would be
        too much clutter. For example, changes in large numerical
        quantities such as timestamps, execution times or dates.
    * *new* Use the corresponding line from the `new` file. This is
        useful when one is interested in where there were changes, but
        needs accurate context information to make sense of the
        change.
    * *old* Use the corresponding line from the `old` file. See the
        description for *new*.

\--context-tokenization=CTOK
:   Select the tokenization rules for context lines.
    These options apply to the *wdiff*, *cc* and *ccwide* context
    formats. Possible values are

    * *word* Tokenize by word boundaries (i.e. the regex anchor
    `\\b`). This is the more readable choice and is therefore the
    default. When tokenizing the line into words, the *wdiff* format
    will behave similarly to the `wdiff` command. Importantly, the
    *cc* format will have a better chance of ignoring insignificant
    edits in multiple parts of a large "word", such as multiple digits
    of an address or floating-point number and producing more readable
    output.
    * *char* Consider each character as an individual token. This will
    produce more accurate output which, however, is likely to be too
    cluttered for general use.

\--mark-changed-context
:   Prefix each changed context line with a bang (`!`) character. This
    can be useful when using `--context-format=new` (or `old`) to be
    informed of which context lines have changes between files, even
    when those changes are not being displayed.

\--display-selected
:   Output the parts of the input lines that were actually considered
    for comparison, instead of outputting the corresponding lines from
    the input files. This is intended as a diagnostic option for
    debugging any unexpected mismatches of the provided regular
    expressions.

-V, \--version
:   Print version information

-h, \--help
:   Display usage

# NOTES

Input is treated as arbitrary bytes. That means that it does not need
to be of a valid encoding. Conversely, unicode character classes are
not available when specifying a regular expression.

If neither `-r` nor `-i` are specified, `subdiff` will behave as
`diff`.

# EXIT STATUS

Like `diff`, `subdiff` terminates with exit code 0 if there were no
differences between the (selected parts of the) two files. It exits
with 1 if there were differences and with 2 if there was an error.

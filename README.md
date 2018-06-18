# subdiff

`subdiff` is a `diff`-like utility that will apply a set of regular
expression to the input lines, calculate the differences using the
selected part of each line, then output the differences in terms of
the original input lines.

## Motivation

When dealing with execution traces or any kind of structured
serialized output, it's often the case that one is only interested in
changes in select parts of the data. That's usually because there's
some kind of temporal variability in the output (e.g. timestamps or
sequence numbers) or semi-random values (e.g. id numbers or input
data).

However, the context itself, both intra-line (the ignored parts of the
line) and inter-line (the context lines) might very well be
significant for interpreting the differences. For instance, when a
sequence of operations occasionally fails to complete in time, the
timestamps around the changes in the execution might be useful in
order to diagnose the issue (doubly so when one needs to correlate whe
output with concurrent output on a different system).

## Example output

Let's say we want to view the differences between the boot of two
different versions of the `Xen` hypervisor, ignoring the timestamps
for comparison, but still seeing them in the final output.

By default, `subdiff` provides accurate information on the
differences.

For example, say you want to compare two logfiles of a system
boot. Running `journalctl -b N` produces lines of the form:

```
Jun 12 17:59:27 dom0 kernel: x86/fpu: Supporting XSAVE feature 0x001: 'x87 floating point registers'
```

Comparing two months-apart boot logs produces output like (trimmed to
part of one hunk):

```
$ subdiff  -r "^.*:.*:.*:(.*)" boot.old boot.new
[...]
@@ -49,26 +51,27 @@
 {-Nov}{+Jun} {-09}{+12} {-20}{+17}:{-45}{+59}:{-39}{+27} {-localhost}{+dom0} kernel: x86/PAT: MTRRs disabled, skipping PAT initialization too.
 {-Nov}{+Jun} {-09}{+12} {-20}{+17}:{-45}{+59}:{-39}{+27} {-localhost}{+dom0} kernel: x86/PAT: Configuration [0-7]: WB  WT  UC- UC  WC  WP  UC  UC  
 {-Nov}{+Jun} {-09}{+12} {-20}{+17}:{-45}{+59}:{-39}{+27} {-localhost}{+dom0} kernel: e820: last_pfn = 0x9cf00 max_arch_pfn = 0x400000000
-Nov 09 20:45:39 localhost kernel: Scanning 1 areas for low memory corruption
[...]
```

Well. A lot has changed. What if the timestamps aren't really that
important? `subdiff` comes with another mode that preserves the format
of "common" lines: summarization by character class.

```
$ subdiff --context-format=cc  -r "^.*:.*:.*:(.*)" boot.old boot.new
[...]
@@ -49,26 +51,27 @@
 \a+ \d+ \d+:\d+:\d+ \w+ kernel: x86/PAT: MTRRs disabled, skipping PAT initialization too.
 \a+ \d+ \d+:\d+:\d+ \w+ kernel: x86/PAT: Configuration [0-7]: WB  WT  UC- UC  WC  WP  UC  UC  
 \a+ \d+ \d+:\d+:\d+ \w+ kernel: e820: last_pfn = 0x9cf00 max_arch_pfn = 0x400000000
-Nov 09 20:45:39 localhost kernel: Scanning 1 areas for low memory corruption
[...]
```

OK, now, let's look at two `strace -T` (i.e. "record time spent in
system calls") runs of ls listing /opt and ls listing /media (both of
which are empty):

```
$ diff -u ls.opt ls.media | diffstat
 ls.media |  318 +++++++++++++++++++++++++++++++--------------------------------
 1 file changed, 159 insertions(+), 159 deletions(-)
```

Whereas, selecting only the part of the line before the `strace`
timestamps and ignoring addresses, time structs and return values that
are not errors:

```
$ subdiff  -r "(.*)<\d+.+>$" -i '(0x[a-f0-9]+)|(tv_[un]?sec=\d+)|(=\s\d+\s)' ls.opt ls.media  | diffstat
 ls.media |    6 +++---
 1 file changed, 3 insertions(+), 3 deletions(-)
```
Indeed, it's only the arguments to `execve`, `stat` and `open` that
are different.

A final example of looking at differences in a "spatial"
format. Comparing the disassembly of a simple binary before and
after a small arbitrary modification:

```
$ subdiff --context-format=cc  -r "^\s+[0-9a-f]+:\t.*\t(.*)$" -r "^[0-9a-f]+\s(.*)$" -i '(0x[a-f0-9]+)|(#.*)|([0-9a-f]+\s<)|(^\s+[0-9a-f]+:\t00.*)|(.*file format.*)' old new
[...]
@@ -117,14 +117,16 @@
   40053a:      ba \d+ \d+ \d+ \d+              mov    $\w+,%edx
   40053f:      89 c8                   mov    %ecx,%eax
   400541:      f7 ea                   imul   %edx
-  400543:      d1 fa                   sar    %edx
+  400543:      8d 04 0a                lea    (%rdx,%rcx,1),%eax
+  400546:      c1 f8 02                sar    $0x2,%eax
+  400549:      89 c2                   mov    %eax,%edx
   \w+: 89 c8                   mov    %ecx,%eax
   \w+: c1 f8 1f                sar    $0x1f,%eax
   \w+: 29 c2                   sub    %eax,%edx
   \w+: 89 d0                   mov    %edx,%eax
   \w+: 89 c2                   mov    %eax,%edx
   \d+: c1 e2 \d+               shl    $\w+,%edx
-  400553:      01 c2                   add    %eax,%edx
+  400559:      29 c2                   sub    %eax,%edx
   \w+: 89 c8                   mov    %ecx,%eax
   \w+: 29 d0                   sub    %edx,%eax
   \w+: eb 1b                   jmp    \w+ <foo+\w+>
[...]
```

Note: you cannot compare binaries like that except in the simplest of
cases.

Under the right circumstances though, i.e. when each part of
the input can be described by a regular grammar, `subdiff` can give a
quick overview of the actual changes. Notice that, in the example
above, we needed to use two regular expressions: one for lines printed
for function symbols and another for disassembled instructions.

## Features
- `diff`-compatible output (e.g. can be piped to diffstat)
- intelligent handling of context

Additional information can be found in the [manpage](doc/subdiff.md).

## Limitations
- `subdiff` is not as featureful as diff. Importantly, it only makes
  use of a single diff algorithm: the longest common subsequence
  algorithm as implemented by the `lcs-diff` crate.
- `subdiff` keeps both input files in memory at once. This limitation
  could be lifted, although that will require breaking changes to
  `lcs-diff`.
- the backend regex engine (`regex`) does not support lookaround,
  which means it's impossible to express concepts like "ignore the
  characters between pairs of quotes, but do not take into account
  backslash-escaped quotes while in the quoted string".

//! A translator that never answers, to test that Verba does not wait for ever: it reads what it is sent and says nothing.
//! `VERBA_TRANSLATE_EXE=target\mt\release\examples\hang.exe` makes a development copy of Verba use it.

use std::io::Read;

fn main() {
    let mut sink = [0u8; 4096];
    let mut input = std::io::stdin().lock();
    while input.read(&mut sink).unwrap_or(0) > 0 {}
    // the pipe was closed (Verba stopped this program, or quit)
}

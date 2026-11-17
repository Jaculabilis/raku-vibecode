//! Menagerie: one bytecode for Lox, Monkey and Tiger.
//!
//! * [`frontend`] compiles source files of each language to a [`bytecode::Program`].
//! * [`bytecode::Program::link`] joins programs; modules run in order and
//!   share one global namespace, which is how code in different languages
//!   calls each other.
//! * [`vm::Vm`] is the reference interpreter.

pub mod bytecode;
pub mod emit;
pub mod frontend;
pub mod natives;
pub mod value;
pub mod vm;

use std::io::{BufReader, BufWriter};

/// Exit status for compile errors (sysexits EX_DATAERR, as jlox uses).
pub const EXIT_COMPILE_ERROR: i32 = 65;
/// Exit status for runtime errors (sysexits EX_SOFTWARE, as jlox uses).
pub const EXIT_RUNTIME_ERROR: i32 = 70;
/// Exit status for bad command lines.
pub const EXIT_USAGE: i32 = 64;

/// Verify and run a program on the process's standard streams, returning
/// the exit status.  Runs on a thread with a large stack so that dropping
/// long chains of reference-counted values cannot overflow the stack.
pub fn run_program(prog: bytecode::Program) -> i32 {
    if let Err(e) = prog.verify() {
        eprintln!("invalid bytecode: {e}");
        return EXIT_COMPILE_ERROR;
    }
    let worker = std::thread::Builder::new().stack_size(1 << 30).spawn(move || {
        let io = natives::Io {
            out: Box::new(BufWriter::new(std::io::stdout())),
            input: Box::new(BufReader::new(std::io::stdin())),
        };
        let mut vm = vm::Vm::new(prog, io);
        let status = match vm.run() {
            Ok(vm::Outcome::Finished) => 0,
            Ok(vm::Outcome::Exit(code)) => code,
            Err(e) => {
                eprintln!("{}", e.render());
                EXIT_RUNTIME_ERROR
            }
        };
        // The process is about to exit; skip tearing down the heap.
        std::mem::forget(vm);
        status
    });
    match worker.map(|w| w.join()) {
        Ok(Ok(status)) => status,
        _ => {
            eprintln!("internal error: interpreter thread failed");
            EXIT_RUNTIME_ERROR
        }
    }
}

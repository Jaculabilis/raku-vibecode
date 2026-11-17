//! menagerie-vm: the reference interpreter for menagerie bytecode.

use menagerie::bytecode::Program;
use menagerie::{run_program, EXIT_COMPILE_ERROR, EXIT_USAGE};
use std::process::exit;

const USAGE: &str = "\
usage: menagerie-vm [-S] FILE.mbc...

Runs bytecode produced by menageriec.  Several files are linked together and
their modules run in order, sharing one global namespace.

options:
  -S, --disasm   print a disassembly instead of running
  -h, --help     show this help";

fn main() {
    let mut disasm = false;
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-S" | "--disasm" => disasm = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            s if s.starts_with('-') => {
                eprintln!("menagerie-vm: unknown option '{s}'\n\n{USAGE}");
                exit(EXIT_USAGE);
            }
            _ => files.push(a),
        }
    }
    if files.is_empty() {
        eprintln!("{USAGE}");
        exit(EXIT_USAGE);
    }
    let mut parts = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap_or_else(|e| {
            eprintln!("{f}: {e}");
            exit(EXIT_USAGE)
        });
        match Program::from_bytes(&bytes) {
            Ok(p) => parts.push(p),
            Err(e) => {
                eprintln!("{f}: {e}");
                exit(EXIT_COMPILE_ERROR);
            }
        }
    }
    let prog = Program::link(parts);
    if disasm {
        if let Err(e) = prog.verify() {
            eprintln!("invalid bytecode: {e}");
            exit(EXIT_COMPILE_ERROR);
        }
        print!("{prog}");
        return;
    }
    exit(run_program(prog));
}

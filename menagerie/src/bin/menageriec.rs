//! menageriec: compile and link Lox, Monkey and Tiger sources (and
//! existing bytecode files) into one bytecode program.

use menagerie::bytecode::{Lang, Program};
use menagerie::frontend;
use menagerie::{run_program, EXIT_COMPILE_ERROR, EXIT_USAGE};
use std::process::exit;

const USAGE: &str = "\
usage: menageriec [options] FILE...

Compiles each FILE and links them into one bytecode program whose modules
run in the order given, sharing one global namespace.

The language of each file is taken from its extension (.lox; .monkey, .mon
or .mky; .tig or .tiger) unless overridden with -x.  Files ending in .mbc are
linked in as already-compiled bytecode.

options:
  -o FILE        write the bytecode to FILE (default: out.mbc)
  -x LANG        treat the following source files as LANG (lox, monkey, tiger)
  -S, --disasm   print a disassembly of the linked program
  -r, --run      run the program after compiling (no file is written unless
                 -o is also given)
  -h, --help     show this help";

fn main() {
    // The parsers are recursive; give pathological nesting plenty of room.
    let compiler = std::thread::Builder::new().stack_size(1 << 30).spawn(real_main);
    match compiler.map(|t| t.join()) {
        Ok(Ok(())) => {}
        _ => {
            eprintln!("menageriec: internal error");
            exit(EXIT_COMPILE_ERROR);
        }
    }
}

fn real_main() {
    let mut args = std::env::args().skip(1);
    let mut out: Option<String> = None;
    let mut forced: Option<Lang> = None;
    let mut disasm = false;
    let mut run = false;
    let mut inputs: Vec<(String, Option<Lang>)> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = Some(args.next().unwrap_or_else(|| usage("-o needs a file name"))),
            "-x" => {
                let l = args.next().unwrap_or_else(|| usage("-x needs a language"));
                forced = Some(frontend::lang_by_name(&l).unwrap_or_else(|| usage(&format!("unknown language '{l}'"))));
            }
            "-S" | "--disasm" => disasm = true,
            "-r" | "--run" => run = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            s if s.starts_with('-') && s != "-" => usage(&format!("unknown option '{s}'")),
            _ => inputs.push((a, forced)),
        }
    }
    if inputs.is_empty() {
        usage("no input files");
    }

    let mut parts = Vec::new();
    let mut failed = false;
    for (path, forced) in &inputs {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: {e}");
                exit(EXIT_USAGE);
            }
        };
        if forced.is_none() && path.ends_with(".mbc") {
            match Program::from_bytes(&bytes) {
                Ok(p) => parts.push(p),
                Err(e) => {
                    eprintln!("{path}: {e}");
                    failed = true;
                }
            }
            continue;
        }
        let Some(lang) = forced.or_else(|| frontend::lang_for_path(path)) else {
            eprintln!("{path}: cannot tell the language from the file name; use -x LANG");
            exit(EXIT_USAGE);
        };
        let Ok(src) = String::from_utf8(bytes) else {
            eprintln!("{path}: source is not valid UTF-8");
            failed = true;
            continue;
        };
        match frontend::compile(lang, &src, path) {
            Ok(p) => parts.push(p),
            Err(errs) => {
                for e in errs {
                    eprintln!("{e}");
                }
                failed = true;
            }
        }
    }
    if failed {
        exit(EXIT_COMPILE_ERROR);
    }

    let prog = Program::link(parts);
    if let Err(e) = prog.verify() {
        eprintln!("internal error: compiler produced invalid bytecode: {e}");
        exit(EXIT_COMPILE_ERROR);
    }
    if disasm {
        print!("{prog}");
    }
    if out.is_some() || !run {
        let path = out.unwrap_or_else(|| "out.mbc".to_string());
        if let Err(e) = std::fs::write(&path, prog.to_bytes()) {
            eprintln!("{path}: {e}");
            exit(EXIT_USAGE);
        }
    }
    if run {
        exit(run_program(prog));
    }
}

fn usage(msg: &str) -> ! {
    eprintln!("menageriec: {msg}\n\n{USAGE}");
    exit(EXIT_USAGE);
}
